// Copyright 2026 BoxLite AI
// SPDX-License-Identifier: AGPL-3.0

package boxlite

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	sdk "github.com/boxlite-ai/boxlite/sdks/go"
	"github.com/boxlite-ai/runner/cmd/runner/config"
	"github.com/boxlite-ai/runner/pkg/api/dto"
)

// A REST-backed SDK exercises the real Client and FFI without booting a VM.
// Hold one export in the backend while calling another Client operation.
func operationTestClient(t *testing.T) (*Client, <-chan struct{}, func()) {
	t.Helper()
	t.Setenv("BOXLITE_API_URL", "http://runner.invalid")
	t.Setenv("BOXLITE_RUNNER_TOKEN", "YOUR_API_KEY")
	t.Setenv("RUNNER_DOMAIN", "localhost")
	if _, err := config.GetConfig(); err != nil {
		t.Fatal(err)
	}
	entered, release := make(chan struct{}), make(chan struct{})
	var once sync.Once
	unblock := func() { once.Do(func() { close(release) }) }
	var exports atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		switch {
		case r.URL.Path == "/v1/config":
			io.WriteString(w, `{"capabilities":{"export_enabled":true,"import_enabled":true}}`)
		case strings.HasSuffix(r.URL.Path, "/export"):
			if exports.Add(1) == 1 {
				close(entered)
				<-release
			}
			w.Header().Set("Content-Type", "application/octet-stream")
			io.WriteString(w, "archive")
		case (r.Method == http.MethodGet && strings.HasPrefix(r.URL.Path, "/v1/boxes/")) || strings.HasSuffix(r.URL.Path, "/start") || strings.HasSuffix(r.URL.Path, "/stop"):
			id := strings.Split(strings.TrimPrefix(r.URL.Path, "/v1/boxes/"), "/")[0]
			fmt.Fprintf(w, `{"box_id":%q,"name":%q,"status":"running","created_at":"2026-09-11T00:00:00Z","updated_at":"2026-09-11T00:00:00Z","pid":null,"image":"test-image","cpus":1,"memory_mib":256}`, id, id)
		case r.Method == http.MethodDelete:
			io.WriteString(w, `{}`)
		default:
			t.Errorf("unexpected SDK request: %s %s", r.Method, r.URL.Path)
			http.NotFound(w, r)
		}
	}))
	t.Cleanup(server.Close)
	runtime, err := sdk.NewRest(sdk.BoxliteRestOptions{URL: server.URL})
	if err != nil {
		t.Fatal(err)
	}
	client := &Client{runtime: runtime, logger: slog.New(slog.NewTextHandler(io.Discard, nil)), homeDir: t.TempDir(), boxes: make(map[string]*sdk.Box), volumeCleanup: volumeCleanupConfig{interval: time.Hour, dryRun: true}, lastVolumeCleanup: time.Now()}
	t.Cleanup(func() { _ = client.Close() })
	t.Cleanup(unblock)
	return client, entered, unblock
}

func holdExport(t *testing.T, client *Client, entered <-chan struct{}, unblock func()) {
	t.Helper()
	dest := t.TempDir()
	finished := make(chan error, 1)
	go func() {
		_, err := client.ExportBox(context.Background(), "pr2-operation-fixture-a", dest)
		finished <- err
	}()
	t.Cleanup(func() {
		unblock()
		select {
		case err := <-finished:
			if err != nil {
				t.Errorf("export: %v", err)
			}
		case <-time.After(5 * time.Second):
			t.Error("export did not finish")
		}
	})
	select {
	case <-entered:
	case <-time.After(5 * time.Second):
		t.Fatal("export did not reach backend")
	}
}

func TestBoxOperationsWaitForSameBox(t *testing.T) {
	operations := map[string]func(context.Context, *Client) error{
		"export": func(ctx context.Context, c *Client) error {
			_, err := c.ExportBox(ctx, "pr2-operation-fixture-a", t.TempDir())
			return err
		},
		"start": func(ctx context.Context, c *Client) error {
			_, err := c.Start(ctx, "pr2-operation-fixture-a", nil, nil)
			return err
		},
		"stop":    func(ctx context.Context, c *Client) error { return c.Stop(ctx, "pr2-operation-fixture-a", false) },
		"destroy": func(ctx context.Context, c *Client) error { return c.Destroy(ctx, "pr2-operation-fixture-a") },
		"create": func(ctx context.Context, c *Client) error {
			skipStart := true
			_, _, err := c.Create(ctx, dto.CreateBoxDTO{Id: "pr2-operation-fixture-a", Image: "test-image", SkipStart: &skipStart})
			return err
		},
		"import": func(ctx context.Context, c *Client) error {
			return c.ImportBox(ctx, "pr2-operation-fixture-a", "unused.boxlite")
		},
		"recover": func(ctx context.Context, c *Client) error {
			return c.RecoverBox(ctx, "pr2-operation-fixture-a", dto.RecoverBoxDTO{})
		},
	}
	for name, operation := range operations {
		t.Run(name, func(t *testing.T) {
			client, entered, unblock := operationTestClient(t)
			holdExport(t, client, entered, unblock)
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			if err := operation(ctx, client); !errors.Is(err, context.DeadlineExceeded) {
				t.Fatalf("%s entered the same box while export was active: got %v, want deadline exceeded waiting for the box", name, err)
			}
			unblock()
			ctx, cancel = context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			if err := operation(ctx, client); err != nil {
				t.Fatalf("%s after export: %v", name, err)
			}
		})
	}
}

func TestBoxOperationsKeepOtherBoxesAndRunnersIndependent(t *testing.T) {
	client, entered, unblock := operationTestClient(t)
	holdExport(t, client, entered, unblock)
	otherRunner, _, _ := operationTestClient(t)
	for _, target := range []struct {
		client *Client
		id     string
	}{{client, "pr2-operation-fixture-b"}, {otherRunner, "pr2-operation-fixture-a"}} {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		_, err := target.client.Start(ctx, target.id, nil, nil)
		cancel()
		if err != nil {
			t.Fatalf("independent box %s blocked: %v", target.id, err)
		}
	}
}

func TestCancelledBoxOperationKeepsOwner(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		var operations boxOperations
		release, err := operations.acquire(context.Background(), "box")
		if err != nil {
			t.Fatal(err)
		}
		result := make(chan error, 1)
		wait := func(ctx context.Context) {
			unlock, err := operations.acquire(ctx, "box")
			if err == nil {
				unlock()
			}
			result <- err
		}
		ctx, cancel := context.WithCancel(context.Background())
		go wait(ctx)
		synctest.Wait()
		cancel()
		if err := <-result; !errors.Is(err, context.Canceled) {
			t.Fatalf("cancelled wait: %v", err)
		}
		go wait(context.Background())
		synctest.Wait()
		select {
		case <-result:
			t.Fatal("cancelled waiter released the owner's gate")
		default:
		}
		release()
		if err := <-result; err != nil {
			t.Fatal(err)
		}
		if len(operations.active) != 0 {
			t.Fatal("completed operation left a gate behind")
		}
	})
}
