// Copyright 2026 BoxLite AI
// SPDX-License-Identifier: AGPL-3.0

package proxy

import (
	"bufio"
	"context"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	common_cache "github.com/boxlite-ai/common-go/pkg/cache"
	common_proxy "github.com/boxlite-ai/common-go/pkg/proxy"
	"github.com/gin-gonic/gin"
)

type closeWriteConn struct {
	net.Conn
	called bool
}

func TestConnectCapacityRejectsBeforeDispatchAndReleases(t *testing.T) {
	entered := make(chan struct{}, 2)
	release := make(chan struct{})
	defer close(release)
	var calls atomic.Int32
	var wg sync.WaitGroup
	handler := connectAwareHandler(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) <= 2 {
			entered <- struct{}{}
			select {
			case <-release:
			case <-r.Context().Done():
			}
		}
		w.WriteHeader(http.StatusOK)
	}), http.NotFoundHandler(), &wg, 2)
	done := make(chan struct{}, 2)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	for range 2 {
		go func() {
			handler.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil).WithContext(ctx))
			done <- struct{}{}
		}()
	}
	for range 2 {
		select {
		case <-entered:
		case <-time.After(time.Second):
			t.Fatal("initial CONNECT was not dispatched")
		}
	}
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil))
	if response.Code != http.StatusServiceUnavailable || calls.Load() != 2 {
		t.Fatalf("over capacity: status=%d dispatches=%d; want 503 and 2", response.Code, calls.Load())
	}
	cancel()
	for range 2 {
		select {
		case <-done:
		case <-time.After(time.Second):
			t.Fatal("cancelled CONNECT did not return")
		}
	}
	response = httptest.NewRecorder()
	handler.ServeHTTP(response, httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil))
	if response.Code != http.StatusOK || calls.Load() != 3 {
		t.Fatalf("capacity was not released: status=%d dispatches=%d", response.Code, calls.Load())
	}
}

func TestTunnelCapacityResponseReachesConnectAndPreview(t *testing.T) {
	for _, runnerStatus := range []int{http.StatusServiceUnavailable, http.StatusBadGateway} {
		runner := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if r.Method != http.MethodConnect {
				t.Errorf("runner received %s, want CONNECT", r.Method)
			}
			http.Error(w, "runner failure", runnerStatus)
		}))
		defer runner.Close()
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		public := common_cache.NewMapCache[bool](ctx)
		runners := common_cache.NewMapCache[RunnerInfo](ctx)
		if err := public.Set(ctx, "AbCdEf123456", true, time.Minute); err != nil {
			t.Fatal(err)
		}
		if err := runners.Set(ctx, "AbCdEf123456", RunnerInfo{ApiUrl: runner.URL}, time.Minute); err != nil {
			t.Fatal(err)
		}
		p := &Proxy{boxPublicCache: public, boxRunnerCache: runners}
		request := httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil)
		request.Host = "3000-d-416243644566313233343536.proxy.test"
		response := httptest.NewRecorder()
		p.handleTunnelConnect(response, request)
		if response.Code != runnerStatus {
			t.Errorf("CONNECT: runner status %d became %d", runnerStatus, response.Code)
		}

		transport := p.newGuestPortTransport()
		defer transport.CloseIdleConnections()
		target, _ := url.Parse("http://AbCdEf123456:3000/")
		router := gin.New()
		router.GET("/", common_proxy.NewProxyRequestHandler(func(*gin.Context) (*common_proxy.RequestTarget, error) {
			return &common_proxy.RequestTarget{URL: target, Transport: transport}, nil
		}, nil))
		response = httptest.NewRecorder()
		router.ServeHTTP(response, httptest.NewRequest(http.MethodGet, "http://preview.test/", nil).WithContext(ctx))
		if response.Code != runnerStatus {
			body, _ := io.ReadAll(response.Result().Body)
			t.Errorf("preview: runner status %d became %d (%s)", runnerStatus, response.Code, body)
		}
	}
}

func (c *closeWriteConn) CloseWrite() error {
	c.called = true
	return nil
}

func TestConnectAuthorityBypassesHTTPRouter(t *testing.T) {
	matched := false
	shutdownWg := &sync.WaitGroup{}
	handler := connectAwareHandler(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		matched = true
		writer.WriteHeader(http.StatusProxyAuthRequired)
	}), http.NotFoundHandler(), shutdownWg, 2)

	request := httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil)
	request.RequestURI = "proxy.test:443"
	request.URL.Path = ""
	handler.ServeHTTP(httptest.NewRecorder(), request)

	if !matched {
		t.Fatal("authority-form CONNECT did not reach the tunnel handler")
	}
}

func TestConnectHandlerTracksTunnelForShutdown(t *testing.T) {
	started := make(chan struct{})
	release := make(chan struct{})
	shutdownWg := &sync.WaitGroup{}
	handler := connectAwareHandler(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		close(started)
		<-release
	}), http.NotFoundHandler(), shutdownWg, 2)

	request := httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil)
	done := make(chan struct{})
	go func() {
		handler.ServeHTTP(httptest.NewRecorder(), request)
		close(done)
	}()
	<-started

	shutdownDone := make(chan struct{})
	go func() {
		shutdownWg.Wait()
		close(shutdownDone)
	}()
	select {
	case <-shutdownDone:
		t.Fatal("shutdown completed while CONNECT tunnel was active")
	case <-time.After(20 * time.Millisecond):
	}

	close(release)
	select {
	case <-shutdownDone:
	case <-time.After(time.Second):
		t.Fatal("shutdown did not complete after CONNECT tunnel closed")
	}
	<-done
}

func TestTunnelTargetUsesPreviewAuthority(t *testing.T) {
	request := httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil)
	request.Host = "3000-d-416243644566313233343536.proxy.test:443"

	boxID, port, err := (&Proxy{}).tunnelTarget(request)
	if err != nil {
		t.Fatal(err)
	}
	if boxID != "AbCdEf123456" || port != 3000 {
		t.Fatalf("unexpected tunnel target: %s:%d", boxID, port)
	}
}

func TestTunnelConnectRejectsPrivateBoxBeforeRunnerDial(t *testing.T) {
	ctx := context.Background()
	publicCache := common_cache.NewMapCache[bool](ctx)
	if err := publicCache.Set(ctx, "AbCdEf123456", false, time.Minute); err != nil {
		t.Fatal(err)
	}
	proxy := &Proxy{boxPublicCache: publicCache}
	request := httptest.NewRequest(http.MethodConnect, "http://proxy.test", nil)
	request.Host = "3000-d-416243644566313233343536.proxy.test:443"
	response := httptest.NewRecorder()

	proxy.handleTunnelConnect(response, request)

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusForbidden)
	}
}

func TestBufferedConnForwardsCloseWrite(t *testing.T) {
	conn, peer := net.Pipe()
	defer conn.Close()
	defer peer.Close()
	tracked := &closeWriteConn{Conn: conn}
	buffered := common_proxy.NewBufferedConn(tracked, bufio.NewReader(conn))

	closeWriter, ok := buffered.(interface{ CloseWrite() error })
	if !ok {
		t.Fatal("buffered connection does not support CloseWrite")
	}
	if err := closeWriter.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	if !tracked.called {
		t.Fatal("CloseWrite was not forwarded")
	}
}
