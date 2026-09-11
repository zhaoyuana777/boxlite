package api

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/pem"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/boxlite-ai/runner/cmd/runner/config"
)

func newTestServer(t *testing.T) *ApiServer {
	t.Helper()
	t.Setenv("BOXLITE_API_URL", "http://127.0.0.1")
	t.Setenv("BOXLITE_RUNNER_TOKEN", "test-runner-token")
	t.Setenv("RUNNER_DOMAIN", "localhost")
	if _, err := config.GetConfig(); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", ":0")
	if err != nil {
		t.Fatal(err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	listener.Close()
	return NewApiServer(ApiServerConfig{
		Logger:  slog.New(slog.NewTextHandler(io.Discard, nil)),
		ApiPort: port,
	})
}

func startServerWithinDeadline(t *testing.T, server *ApiServer) (<-chan error, error) {
	t.Helper()
	type result struct {
		exit <-chan error
		err  error
	}
	started := make(chan result, 1)
	go func() {
		exit, err := server.Start(context.Background())
		started <- result{exit: exit, err: err}
	}()
	select {
	case result := <-started:
		return result.exit, result.err
	case <-time.After(2 * time.Second):
		t.Fatal("Start did not return after binding the API listener")
		return nil, nil
	}
}

func TestStartReturnsWithServingListener(t *testing.T) {
	for _, useTLS := range []bool{false, true} {
		name := "HTTP"
		if useTLS {
			name = "HTTPS"
		}
		t.Run(name, func(t *testing.T) {
			server := newTestServer(t)
			transport := &http.Transport{}
			t.Cleanup(transport.CloseIdleConnections)
			scheme := "http"
			if useTLS {
				fixture := httptest.NewTLSServer(http.NotFoundHandler())
				certificate := fixture.TLS.Certificates[0]
				roots := x509.NewCertPool()
				roots.AddCert(fixture.Certificate())
				fixture.Close()
				key, err := x509.MarshalPKCS8PrivateKey(certificate.PrivateKey)
				if err != nil {
					t.Fatal(err)
				}
				server.enableTLS = true
				server.tlsCertFile = filepath.Join(t.TempDir(), "certificate.pem")
				server.tlsKeyFile = filepath.Join(t.TempDir(), "key.pem")
				if err := os.WriteFile(server.tlsCertFile, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: certificate.Certificate[0]}), 0600); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(server.tlsKeyFile, pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: key}), 0600); err != nil {
					t.Fatal(err)
				}
				transport.TLSClientConfig = &tls.Config{RootCAs: roots}
				scheme = "https"
			}

			exit, err := startServerWithinDeadline(t, server)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(server.Stop)
			if useTLS {
				// Startup must retain the loaded key pair, not reopen these files
				// after reporting success to the caller.
				if err := os.Remove(server.tlsCertFile); err != nil {
					t.Fatal(err)
				}
				if err := os.Remove(server.tlsKeyFile); err != nil {
					t.Fatal(err)
				}
			}
			client := &http.Client{Transport: transport, Timeout: 2 * time.Second}
			response, err := client.Get(scheme + "://127.0.0.1" + server.httpServer.Addr + "/")
			if err != nil {
				t.Fatal(err)
			}
			response.Body.Close()
			if response.StatusCode != http.StatusOK {
				t.Fatalf("health endpoint returned %d", response.StatusCode)
			}

			server.Stop()
			select {
			case err := <-exit:
				if !errors.Is(err, http.ErrServerClosed) {
					t.Fatalf("shutdown returned %v", err)
				}
			case <-time.After(2 * time.Second):
				t.Fatal("Serve did not report shutdown")
			}
			select {
			case _, open := <-exit:
				if open {
					t.Fatal("Serve reported more than one exit result")
				}
			case <-time.After(2 * time.Second):
				t.Fatal("Serve did not close its exit channel")
			}
		})
	}
}

func TestStartRejectsInvalidTLSWithoutBinding(t *testing.T) {
	server := newTestServer(t)
	server.enableTLS = true
	server.tlsCertFile = filepath.Join(t.TempDir(), "missing.pem")
	server.tlsKeyFile = filepath.Join(t.TempDir(), "missing.key")
	exit, err := startServerWithinDeadline(t, server)
	if err == nil || exit != nil {
		t.Fatalf("invalid TLS must fail startup, got exit=%v error=%v", exit, err)
	}
	listener, err := net.Listen("tcp", server.httpServer.Addr)
	if err != nil {
		t.Fatalf("failed TLS startup left the API port bound: %v", err)
	}
	listener.Close()
}

func TestStartRejectsOccupiedPort(t *testing.T) {
	server := newTestServer(t)
	listener, err := net.Listen("tcp", ":0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	server.apiPort = listener.Addr().(*net.TCPAddr).Port
	exit, err := startServerWithinDeadline(t, server)
	if err == nil || exit != nil {
		t.Fatalf("occupied port must fail startup, got exit=%v error=%v", exit, err)
	}
}
