package controllers

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
)

func TestBoxliteNetworkTunnelRequiresConnect(t *testing.T) {
	gin.SetMode(gin.TestMode)
	recorder := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(recorder)
	ctx.Request = httptest.NewRequest(http.MethodGet, "/v1/boxes/box-1/network/tunnel?port=3000", nil)

	BoxliteNetworkTunnel(slog.Default(), 2, 1)(ctx)

	if recorder.Code != http.StatusMethodNotAllowed {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusMethodNotAllowed)
	}
}

func tunnelTestRouter(handler gin.HandlerFunc) *gin.Engine {
	gin.SetMode(gin.TestMode)
	router := gin.New()
	router.Any("/:boxId/network/tunnel", handler)
	return router
}

func tunnelRequest(box string) *http.Request {
	return httptest.NewRequest(http.MethodConnect, "/"+box+"/network/tunnel?port=3000", nil)
}

func awaitTunnelEvent[T any](t *testing.T, events <-chan T) T {
	t.Helper()
	select {
	case value := <-events:
		return value
	case <-time.After(3 * time.Second):
		t.Fatal("tunnel operation did not finish")
		var zero T
		return zero
	}
}

func TestNetworkTunnelCapacityUsesCanonicalBoxAndReleasesFailedDials(t *testing.T) {
	limits := newTunnelLimits(2, 1)
	entered := make(chan string, 4)
	var lookups, dials atomic.Int32
	router := tunnelTestRouter(networkTunnelHandler(slog.Default(), limits, func(ctx context.Context, id string, _ uint16) (guestTunnelTarget, error) {
		lookups.Add(1)
		if id == "alias" {
			id = "box-a"
		}
		return guestTunnelTarget{boxID: id, dial: func() (net.Conn, error) {
			dials.Add(1)
			entered <- id
			<-ctx.Done()
			return nil, ctx.Err()
		}}, nil
	}))
	start := func(id string) (context.CancelFunc, <-chan int) {
		ctx, cancel := context.WithCancel(context.Background())
		t.Cleanup(cancel)
		done := make(chan int, 1)
		go func() {
			response := httptest.NewRecorder()
			router.ServeHTTP(response, tunnelRequest(id).WithContext(ctx))
			done <- response.Code
		}()
		if got := awaitTunnelEvent(t, entered); got == "" {
			t.Fatal("empty resolved box ID")
		}
		return cancel, done
	}
	cancelA, doneA := start("alias")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, tunnelRequest("box-a"))
	if response.Code != http.StatusServiceUnavailable || dials.Load() != 1 {
		t.Fatalf("alias bypassed Box quota: status=%d dials=%d", response.Code, dials.Load())
	}
	cancelB, doneB := start("box-b")
	before := lookups.Load()
	response = httptest.NewRecorder()
	router.ServeHTTP(response, tunnelRequest("box-c"))
	if response.Code != http.StatusServiceUnavailable || lookups.Load() != before {
		t.Fatalf("global quota did not reject before lookup: status=%d lookups=%d", response.Code, lookups.Load())
	}
	cancelA()
	if got := awaitTunnelEvent(t, doneA); got != http.StatusBadGateway {
		t.Fatalf("failed dial status=%d", got)
	}
	cancelAgain, doneAgain := start("box-a")
	cancelAgain()
	awaitTunnelEvent(t, doneAgain)
	cancelB()
	awaitTunnelEvent(t, doneB)
	if len(limits.slots) != 0 || len(limits.boxes) != 0 {
		t.Fatalf("capacity leaked: total=%d boxes=%v", len(limits.slots), limits.boxes)
	}
}

type failedTunnelHijack struct{ *httptest.ResponseRecorder }

func (failedTunnelHijack) Hijack() (net.Conn, *bufio.ReadWriter, error) {
	return nil, nil, errors.New("injected hijack failure")
}

func TestNetworkTunnelCapacityReleasesLookupAndHijackFailures(t *testing.T) {
	limits := newTunnelLimits(1, 1)
	lookupFails := true
	var peer net.Conn
	router := tunnelTestRouter(networkTunnelHandler(slog.Default(), limits, func(context.Context, string, uint16) (guestTunnelTarget, error) {
		if lookupFails {
			return guestTunnelTarget{}, errors.New("injected lookup failure")
		}
		return guestTunnelTarget{boxID: "box-a", dial: func() (net.Conn, error) {
			var conn net.Conn
			conn, peer = net.Pipe()
			return conn, nil
		}}, nil
	}))
	response := httptest.NewRecorder()
	router.ServeHTTP(response, tunnelRequest("box-a"))
	if response.Code != http.StatusBadGateway || len(limits.slots) != 0 {
		t.Fatal("lookup failure did not release capacity")
	}
	lookupFails = false
	for range 2 {
		router.ServeHTTP(failedTunnelHijack{httptest.NewRecorder()}, tunnelRequest("box-a"))
		if peer == nil {
			t.Fatal("capacity was not reusable")
		}
		_ = peer.SetReadDeadline(time.Now().Add(time.Second))
		if _, err := peer.Read(make([]byte, 1)); err != io.EOF {
			t.Fatalf("guest connection was not closed after hijack failure: %v", err)
		}
		peer.Close()
		peer = nil
	}
	if len(limits.slots) != 0 || len(limits.boxes) != 0 {
		t.Fatal("hijack failure leaked capacity")
	}
}

func TestNetworkTunnelCapacityIsHeldThroughHalfClose(t *testing.T) {
	guest, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer guest.Close()
	readEOF := make(chan error, 1)
	reply := make(chan struct{})
	defer close(reply)
	go func() {
		conn, err := guest.Accept()
		if err != nil {
			readEOF <- err
			return
		}
		defer conn.Close()
		_ = conn.SetDeadline(time.Now().Add(5 * time.Second))
		_, err = io.ReadAll(conn)
		readEOF <- err
		<-reply
		_, _ = io.WriteString(conn, "reply after EOF")
	}()
	limits := newTunnelLimits(2, 1)
	router := tunnelTestRouter(networkTunnelHandler(slog.Default(), limits, func(context.Context, string, uint16) (guestTunnelTarget, error) {
		return guestTunnelTarget{boxID: "box-a", dial: func() (net.Conn, error) {
			return net.Dial("tcp", guest.Addr().String())
		}}, nil
	}))
	done := make(chan struct{}, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		router.ServeHTTP(w, r)
		done <- struct{}{}
	}))
	defer server.Close()
	conn, err := net.Dial("tcp", strings.TrimPrefix(server.URL, "http://"))
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(5 * time.Second))
	_, _ = fmt.Fprint(conn, "CONNECT /box-a/network/tunnel?port=3000 HTTP/1.1\r\nHost: runner.test\r\n\r\n")
	reader := bufio.NewReader(conn)
	response, err := http.ReadResponse(reader, &http.Request{Method: http.MethodConnect})
	if err != nil {
		t.Fatal(err)
	}
	if response.StatusCode != http.StatusOK {
		t.Fatalf("CONNECT status=%d", response.StatusCode)
	}
	if err := conn.(*net.TCPConn).CloseWrite(); err != nil {
		t.Fatal(err)
	}
	if err := awaitTunnelEvent(t, readEOF); err != nil {
		t.Fatal(err)
	}
	refused := httptest.NewRecorder()
	router.ServeHTTP(refused, tunnelRequest("box-a"))
	if refused.Code != http.StatusServiceUnavailable {
		t.Fatalf("half-close released Box capacity: status=%d", refused.Code)
	}
	reply <- struct{}{}
	body, err := io.ReadAll(reader)
	if err != nil || string(body) != "reply after EOF" {
		t.Fatalf("half-close response=%q err=%v", body, err)
	}
	awaitTunnelEvent(t, done)
	if len(limits.slots) != 0 || len(limits.boxes) != 0 {
		t.Fatal("completed tunnel leaked capacity")
	}
}

func TestBoxliteNetworkTunnelRejectsInvalidPortBeforeRuntimeLookup(t *testing.T) {
	gin.SetMode(gin.TestMode)
	recorder := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(recorder)
	ctx.Params = gin.Params{{Key: "boxId", Value: "box-1"}}
	ctx.Request = httptest.NewRequest(http.MethodConnect, "/v1/boxes/box-1/network/tunnel?port=0", nil)

	BoxliteNetworkTunnel(slog.Default(), 2, 1)(ctx)

	if recorder.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusBadRequest)
	}
}
