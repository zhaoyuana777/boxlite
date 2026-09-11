// Copyright 2026 BoxLite AI
// SPDX-License-Identifier: AGPL-3.0

package boxlite

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	sdk "github.com/boxlite-ai/boxlite/sdks/go"
	"github.com/boxlite-ai/runner/cmd/runner/config"
	"github.com/boxlite-ai/runner/pkg/api/dto"
)

func TestRecoverPreservesOriginalBox(t *testing.T) {
	for _, scenario := range []string{"success", "missing", "auto-delete", "stop failure", "start failure", "configured", "missing mount record", "lookup unavailable", "empty mount record", "partial mount record"} {
		t.Run(scenario, func(t *testing.T) {
			t.Setenv("BOXLITE_API_URL", "http://runner.invalid")
			t.Setenv("BOXLITE_RUNNER_TOKEN", "YOUR_API_KEY")
			t.Setenv("RUNNER_DOMAIN", "localhost")
			if _, err := config.GetConfig(); err != nil {
				t.Fatal(err)
			}
			if scenario == "empty mount record" || scenario == "partial mount record" {
				runnerConfig, _ := config.GetConfig()
				previousEnvironment := runnerConfig.Environment
				runnerConfig.Environment = "development"
				t.Cleanup(func() { runnerConfig.Environment = previousEnvironment })
				record := filepath.Join(getVolumeMountRecordDir(), "original-box.json")
				if err := os.MkdirAll(filepath.Dir(record), 0755); err != nil {
					t.Fatal(err)
				}
				f, err := os.OpenFile(record, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { os.Remove(record) })
				paths := []string{}
				if scenario == "partial mount record" {
					paths = []string{filepath.Join(getVolumeMountBasePath(), volumeMountPrefix+"other-volume")}
				}
				err = json.NewEncoder(f).Encode(boxVolumeMountRecord{BoxID: "original-box", Paths: paths})
				f.Close()
				if err != nil {
					t.Fatal(err)
				}
			}
			var mu sync.Mutex
			var writes []string
			state := "failed"
			if scenario == "configured" {
				state = "configured"
			}
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				mu.Lock()
				defer mu.Unlock()
				w.Header().Set("Content-Type", "application/json")
				if r.URL.Path == "/v1/config" {
					io.WriteString(w, `{}`)
					return
				}
				if r.Method != http.MethodGet {
					writes = append(writes, r.Method+" "+r.URL.Path)
				}
				if scenario == "lookup unavailable" {
					w.WriteHeader(http.StatusServiceUnavailable)
					io.WriteString(w, `{"message":"connection refused"}`)
					return
				}
				if scenario == "missing" {
					w.WriteHeader(http.StatusNotFound)
					io.WriteString(w, `{"message":"original box not found"}`)
					return
				}
				switch {
				case strings.HasSuffix(r.URL.Path, "/stop"):
					if scenario == "stop failure" {
						w.WriteHeader(http.StatusInternalServerError)
						io.WriteString(w, `{"message":"stop failed"}`)
						return
					}
					state = "stopped"
				case strings.HasSuffix(r.URL.Path, "/start"):
					if scenario == "start failure" {
						w.WriteHeader(http.StatusInternalServerError)
						io.WriteString(w, `{"message":"no space left on device"}`)
						return
					}
					state = "running"
				case r.Method != http.MethodGet:
					w.WriteHeader(http.StatusConflict)
					io.WriteString(w, `{"message":"recovery must not delete or create"}`)
					return
				}
				autoDelete := 0
				if scenario == "auto-delete" {
					autoDelete = 1
				}
				fmt.Fprintf(w, `{"box_id":"original-runtime-id","name":"original-box","status":%q,"auto_delete":%d,"created_at":"2026-09-11T00:00:00Z","updated_at":"2026-09-11T00:00:00Z","pid":null,"image":"original-image","cpus":2,"memory_mib":256}`, state, autoDelete)
			}))
			defer server.Close()
			runtime, err := sdk.NewRest(sdk.BoxliteRestOptions{URL: server.URL})
			if err != nil {
				t.Fatal(err)
			}
			client := &Client{runtime: runtime, logger: slog.New(slog.NewTextHandler(io.Discard, nil)), homeDir: t.TempDir(), boxes: make(map[string]*sdk.Box), volumeMutexes: make(map[string]*sync.Mutex), volumeCleanup: volumeCleanupConfig{dryRun: true}, lastVolumeCleanup: time.Now()}
			defer client.Close()
			legacy := dto.RecoverBoxDTO{CpuQuota: 99, Env: map[string]string{"IGNORED": "replacement"}}
			if scenario == "missing mount record" || scenario == "empty mount record" || scenario == "partial mount record" {
				legacy.Volumes = []dto.VolumeDTO{{VolumeId: "original-volume", MountPath: "/data"}}
			}
			err = client.RecoverBox(context.Background(), "original-box", legacy)
			mu.Lock()
			defer mu.Unlock()
			for _, write := range writes {
				if !strings.HasSuffix(write, "/stop") && !strings.HasSuffix(write, "/start") {
					t.Fatalf("recovery changed box identity/storage: %s", write)
				}
			}
			wantWrites := 2
			if scenario == "missing" || scenario == "auto-delete" || scenario == "configured" || scenario == "lookup unavailable" {
				wantWrites = 0
			} else if scenario == "stop failure" || (scenario == "missing mount record" || scenario == "empty mount record" || scenario == "partial mount record") {
				wantWrites = 1
			}
			if len(writes) != wantWrites || (err == nil) != (scenario == "success") {
				t.Fatalf("writes=%v, error=%v; want %d writes, success=%v", writes, err, wantWrites, scenario == "success")
			}
			if (scenario == "empty mount record" || scenario == "partial mount record") && !strings.Contains(err.Error(), "mount record") {
				t.Fatalf("expected rejection of incomplete mount record before attempting mounts: %v", err)
			}
			if scenario == "lookup unavailable" && strings.Contains(err.Error(), "not found") {
				t.Fatal("recovery misreported runtime unavailability as a missing box")
			}
		})
	}
}
