package proxy

import (
	"bufio"
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

var errInjectedRead = errors.New("injected read failure")

func TestAcceptConnectHalfCloseDoesNotCancelRequest(t *testing.T) {
	done := make(chan error, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		conn, err := AcceptConnect(w)
		if err != nil {
			done <- err
			return
		}
		defer conn.Close()
		_ = conn.SetDeadline(time.Now().Add(3 * time.Second))
		payload, err := io.ReadAll(conn)
		if err != nil {
			done <- err
			return
		}
		done <- r.Context().Err()
		_, _ = conn.Write(payload)
	}))
	defer server.Close()
	conn, err := net.Dial("tcp", strings.TrimPrefix(server.URL, "http://"))
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(3 * time.Second))
	_, _ = io.WriteString(conn, "CONNECT guest:3000 HTTP/1.1\r\nHost: guest:3000\r\n\r\nrequest")
	reader := bufio.NewReader(conn)
	response, err := http.ReadResponse(reader, &http.Request{Method: http.MethodConnect})
	if err != nil || response.StatusCode != http.StatusOK {
		t.Fatalf("CONNECT failed: response=%v error=%v", response, err)
	}
	if err := conn.(*net.TCPConn).CloseWrite(); err != nil {
		t.Fatal(err)
	}
	if payload, err := io.ReadAll(reader); err != nil || string(payload) != "request" {
		t.Fatalf("response=%q error=%v", payload, err)
	}
	if err := <-done; err != nil {
		t.Fatalf("half-close canceled hijacked request: %v", err)
	}
}

type readErrorConn struct{ net.Conn }

func (c *readErrorConn) Read([]byte) (int, error) { return 0, errInjectedRead }

func tcpConnPair(t *testing.T) (*net.TCPConn, *net.TCPConn) {
	t.Helper()
	listener, err := net.ListenTCP("tcp", &net.TCPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()

	accepted := make(chan *net.TCPConn, 1)
	go func() {
		conn, acceptErr := listener.AcceptTCP()
		if acceptErr == nil {
			accepted <- conn
		}
	}()
	client, err := net.DialTCP("tcp", nil, listener.Addr().(*net.TCPAddr))
	if err != nil {
		t.Fatal(err)
	}
	return client, <-accepted
}

func TestProxyBidirectionalStreamRelaysBothDirections(t *testing.T) {
	client, proxyClient := net.Pipe()
	proxyGuest, guest := net.Pipe()
	defer client.Close()
	defer guest.Close()

	done := make(chan struct{})
	go func() {
		_ = ProxyBidirectionalStream(context.Background(), proxyClient, proxyGuest)
		close(done)
	}()

	for _, exchange := range []struct {
		writer net.Conn
		reader net.Conn
		data   string
	}{{client, guest, "request"}, {guest, client, "response"}} {
		go exchange.writer.Write([]byte(exchange.data))
		payload := make([]byte, len(exchange.data))
		if _, err := io.ReadFull(exchange.reader, payload); err != nil {
			t.Fatal(err)
		}
		if string(payload) != exchange.data {
			t.Fatalf("payload = %q, want %q", payload, exchange.data)
		}
	}

	client.Close()
	guest.Close()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("stream relay did not stop")
	}
}

func TestProxyBidirectionalStreamStopsOnContextCancel(t *testing.T) {
	left, leftPeer := net.Pipe()
	right, rightPeer := net.Pipe()
	defer leftPeer.Close()
	defer rightPeer.Close()

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- ProxyBidirectionalStream(ctx, left, right) }()
	cancel()

	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("error = %v, want context.Canceled", err)
		}
	case <-time.After(time.Second):
		t.Fatal("stream relay did not stop after cancellation")
	}
}

func TestProxyBidirectionalStreamStopsBothDirectionsOnCopyError(t *testing.T) {
	left, leftPeer := net.Pipe()
	right, rightPeer := net.Pipe()
	defer leftPeer.Close()
	defer rightPeer.Close()

	done := make(chan error, 1)
	go func() {
		done <- ProxyBidirectionalStream(context.Background(), &readErrorConn{Conn: left}, right)
	}()

	select {
	case err := <-done:
		if !errors.Is(err, errInjectedRead) {
			t.Fatalf("error = %v, want injected read failure", err)
		}
	case <-time.After(time.Second):
		t.Fatal("stream relay did not stop after copy failure")
	}
}

func TestProxyBidirectionalStreamPreservesHalfCloseResponse(t *testing.T) {
	client, relayLeft := tcpConnPair(t)
	relayRight, guest := tcpConnPair(t)
	defer client.Close()
	defer guest.Close()

	done := make(chan error, 1)
	go func() { done <- ProxyBidirectionalStream(context.Background(), relayLeft, relayRight) }()

	if _, err := client.Write([]byte("request")); err != nil {
		t.Fatal(err)
	}
	if err := client.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	request, err := io.ReadAll(guest)
	if err != nil || string(request) != "request" {
		t.Fatalf("guest request = %q, error = %v", request, err)
	}
	if _, err := guest.Write([]byte("response")); err != nil {
		t.Fatal(err)
	}
	if err := guest.CloseWrite(); err != nil {
		t.Fatal(err)
	}
	response, err := io.ReadAll(client)
	if err != nil || string(response) != "response" {
		t.Fatalf("client response = %q, error = %v", response, err)
	}

	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("stream relay did not stop after both half-closes")
	}
}

func TestBufferedConnPreservesBufferedBytes(t *testing.T) {
	conn, peer := net.Pipe()
	defer conn.Close()
	defer peer.Close()

	reader := bufio.NewReader(conn)
	go peer.Write([]byte("buffered"))
	if _, err := reader.Peek(1); err != nil {
		t.Fatal(err)
	}

	payload := make([]byte, len("buffered"))
	if _, err := io.ReadFull(NewBufferedConn(conn, reader), payload); err != nil {
		t.Fatal(err)
	}
	if string(payload) != "buffered" {
		t.Fatalf("payload = %q", payload)
	}
}
