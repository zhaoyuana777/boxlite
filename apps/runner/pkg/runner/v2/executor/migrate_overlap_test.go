// Copyright 2026 BoxLite AI
// SPDX-License-Identifier: AGPL-3.0

package executor

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"os"
	"sync"
	"testing"
	"time"

	apiclient "github.com/boxlite-ai/boxlite/libs/api-client-go"
	"github.com/boxlite-ai/runner/pkg/storage"
)

type overlappingArchiveStore struct {
	storage.ArchiveStore
	entered chan struct{}
	release chan struct{}
}

func (s *overlappingArchiveStore) Upload(_ context.Context, key, path string) error {
	if key == "first" {
		close(s.entered)
		<-s.release
	}
	_, err := os.ReadFile(path)
	return err
}

func (s *overlappingArchiveStore) Download(_ context.Context, key, path string) error {
	if err := os.WriteFile(path, []byte(key), 0600); err != nil {
		return err
	}
	if key == "first" {
		close(s.entered)
		<-s.release
	}
	return nil
}

type archiveReadingBackend struct{ *fakeBackend }

func (b *archiveReadingBackend) ImportBox(_ context.Context, _, path string) error {
	_, err := os.ReadFile(path)
	return err
}

func TestOverlappingMigrationAttemptsKeepTheirOwnArchive(t *testing.T) {
	for _, jobType := range []apiclient.JobType{apiclient.JOBTYPE_EXPORT_BOX, apiclient.JOBTYPE_IMPORT_BOX} {
		t.Run(string(jobType), func(t *testing.T) {
			store := &overlappingArchiveStore{entered: make(chan struct{}), release: make(chan struct{})}
			e := &Executor{
				log:          slog.New(slog.NewTextHandler(io.Discard, nil)),
				backend:      &archiveReadingBackend{&fakeBackend{calls: &callLog{}, exportedArchiveName: "same-box.boxlite"}},
				archiveStore: store, migrateWorkDir: t.TempDir(),
			}
			run := func(key string) error {
				job := migrateJob(jobType, fmt.Sprintf(`{"arcPath":%q}`, key))
				if jobType == apiclient.JOBTYPE_EXPORT_BOX {
					_, err := e.exportBox(context.Background(), job)
					return err
				}
				_, err := e.importBox(context.Background(), job)
				return err
			}
			first := make(chan error, 1)
			var once sync.Once
			release := func() { once.Do(func() { close(store.release) }) }
			t.Cleanup(release)
			go func() { first <- run("first") }()
			select {
			case <-store.entered:
			case <-time.After(5 * time.Second):
				t.Fatal("first transfer did not start")
			}
			if err := run("second"); err != nil {
				t.Fatalf("second attempt: %v", err)
			}
			release()
			select {
			case err := <-first:
				if err != nil {
					t.Fatalf("second attempt removed first attempt's archive: %v", err)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("first attempt did not finish")
			}
			assertWorkDirEmpty(t, e.migrateWorkDir)
		})
	}
}
