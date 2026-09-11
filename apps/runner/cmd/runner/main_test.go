//go:build darwin || linux

package main

import (
	"bytes"
	"context"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
)

func TestRunnerStartupProcess(t *testing.T) {
	if os.Getenv("BOXLITE_STARTUP_TEST_PROCESS") == "1" {
		os.Exit(run())
	}
}

func TestRunnerWaitsForAPIStartupBeforeHealthAndJobs(t *testing.T) {
	activity := make(chan string, 1)
	controlPlane := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/runners/healthcheck" || strings.HasPrefix(r.URL.Path, "/jobs") {
			select {
			case activity <- r.URL.Path:
			default:
			}
		}
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer controlPlane.Close()

	// A FIFO holds the real TLS loader before it can finish startup. The
	// writer opening confirms that run() reached API initialization.
	certificatePath := filepath.Join(t.TempDir(), "certificate.pem")
	if err := syscall.Mkfifo(certificatePath, 0600); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", ":0")
	if err != nil {
		t.Fatal(err)
	}
	port := listener.Addr().(*net.TCPAddr).Port
	listener.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	command := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestRunnerStartupProcess$")
	command.Env = []string{
		"PATH=" + os.Getenv("PATH"),
		"BOXLITE_STARTUP_TEST_PROCESS=1",
		"BOXLITE_API_URL=" + controlPlane.URL,
		"BOXLITE_RUNNER_TOKEN=test-runner-token",
		"BOXLITE_HOME_DIR=" + t.TempDir(),
		"RUNNER_DOMAIN=localhost",
		"API_VERSION=2",
		"API_PORT=" + strconv.Itoa(port),
		"ENABLE_TLS=true",
		"TLS_CERT_FILE=" + certificatePath,
		"TLS_KEY_FILE=" + filepath.Join(t.TempDir(), "missing.key"),
		"CPU_USAGE_SNAPSHOT_INTERVAL=1s",
		"LOG_LEVEL=error",
	}
	var output bytes.Buffer
	command.Stdout = &output
	command.Stderr = &output
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	exited := make(chan error, 1)
	go func() { exited <- command.Wait() }()

	type openedFile struct {
		file *os.File
		err  error
	}
	opened := make(chan openedFile)
	openDone := make(chan struct{})
	go func() {
		defer close(openDone)
		file, err := os.OpenFile(certificatePath, os.O_WRONLY, 0)
		select {
		case opened <- openedFile{file, err}:
		case <-ctx.Done():
			if file != nil {
				file.Close()
			}
		}
	}()
	defer func() {
		cancel()
		// Unblock the writer if the child failed before opening the FIFO.
		reader, err := os.OpenFile(certificatePath, os.O_RDWR|syscall.O_NONBLOCK, 0)
		if err == nil {
			defer reader.Close()
		}
		<-openDone
	}()
	select {
	case writer := <-opened:
		if writer.err != nil {
			t.Fatal(writer.err)
		}
		defer writer.file.Close()
		select {
		case path := <-activity:
			t.Errorf("runner contacted %s before API startup completed", path)
		case <-time.After(1500 * time.Millisecond):
		}
		// Release startup with invalid TLS input; this must exit without ever
		// starting health reporting or the job poller.
		if _, err := writer.file.WriteString("invalid test certificate"); err != nil {
			t.Fatal(err)
		}
		writer.file.Close()
	case err := <-exited:
		t.Fatalf("runner exited before loading TLS: %v\n%s", err, output.String())
	case <-ctx.Done():
		t.Fatal("runner did not reach TLS initialization")
	}

	select {
	case err := <-exited:
		var exitError *exec.ExitError
		if !errors.As(err, &exitError) || exitError.ExitCode() != 1 {
			t.Fatalf("invalid TLS startup returned %v\n%s", err, output.String())
		}
	case <-ctx.Done():
		t.Fatal("runner did not exit after TLS startup failed")
	}
	select {
	case path := <-activity:
		t.Errorf("runner contacted %s despite failed API startup", path)
	default:
	}
}
