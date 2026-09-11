//go:build boxlite_dev

// Copyright 2026 BoxLite AI
// SPDX-License-Identifier: AGPL-3.0

package boxlite

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	sdk "github.com/boxlite-ai/boxlite/sdks/go"
	"github.com/boxlite-ai/runner/cmd/runner/config"
	"github.com/boxlite-ai/runner/pkg/api/dto"
)

func TestIntegrationRecoverPreservesDisk(t *testing.T) {
	t.Setenv("BOXLITE_API_URL", "http://runner.invalid")
	t.Setenv("BOXLITE_RUNNER_TOKEN", "YOUR_API_KEY")
	t.Setenv("RUNNER_DOMAIN", "localhost")
	if _, err := config.GetConfig(); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	defer cancel()
	client, err := NewClient(ctx, ClientConfig{HomeDir: t.TempDir(), VolumeCleanupDryRun: true})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	opts := []sdk.BoxOption{sdk.WithName("recover-disk"), sdk.WithAutoRemove(false), sdk.WithCPUs(1), sdk.WithMemory(256), sdk.WithEnv("RECOVERY_VALUE", "original")}
	if rootfs := os.Getenv("BOXLITE_TEST_ROOTFS"); rootfs != "" {
		opts = append(opts, sdk.WithRootfsPath(rootfs))
	}
	box, err := client.runtime.Create(ctx, "alpine:latest", opts...)
	if err != nil {
		t.Fatal(err)
	}
	defer client.runtime.ForceRemove(context.Background(), box.ID())
	if err := box.Start(ctx); err != nil {
		t.Fatal(err)
	}
	client.boxes["recover-disk"] = box
	if result, err := client.Exec(ctx, "recover-disk", "sh", "-c", "echo persisted > /root/recovery-marker; sync"); err != nil || result.ExitCode != 0 {
		t.Fatalf("write original disk: result=%v error=%v", result, err)
	}
	before, err := box.Info(ctx)
	if err != nil {
		t.Fatal(err)
	}
	recoverErr := client.RecoverBox(ctx, "recover-disk", dto.RecoverBoxDTO{})
	retained, err := client.runtime.Get(context.Background(), before.ID)
	if err != nil {
		t.Fatalf("recovery removed the original Runtime box: %v", err)
	}
	defer retained.Close()
	if recoverErr != nil {
		t.Fatal(recoverErr)
	}
	recovered, err := client.getOrFetchBox(ctx, "recover-disk")
	if err != nil {
		t.Fatal(err)
	}
	after, err := recovered.Info(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if after.ID != before.ID || after.Name != before.Name || after.CPUs != before.CPUs || after.MemoryMiB != before.MemoryMiB {
		t.Fatal("recovery changed original box identity/configuration")
	}
	result, err := client.Exec(ctx, "recover-disk", "sh", "-c", "cat /root/recovery-marker; printenv RECOVERY_VALUE")
	if err != nil || result.ExitCode != 0 || result.StdOut != "persisted\noriginal\n" {
		t.Fatalf("original disk/configuration lost: result=%v error=%v", result, err)
	}

	// A missing disk must fail without replacing it or deleting the box record.
	if err := client.Stop(ctx, "recover-disk", false); err != nil {
		t.Fatal(err)
	}
	disk := filepath.Join(client.homeDir, "boxes", before.ID, "disks", "disk.qcow2")
	if err := os.Rename(disk, disk+".saved"); err != nil {
		t.Fatal(err)
	}
	if err := client.RecoverBox(ctx, "recover-disk", dto.RecoverBoxDTO{}); err == nil {
		t.Fatal("recovery accepted a missing original disk")
	}
	if _, err := os.Stat(disk); !os.IsNotExist(err) {
		t.Fatal("recovery created a replacement disk")
	}
	if _, err := client.runtime.Get(ctx, before.ID); err != nil {
		t.Fatalf("failed recovery removed the original box: %v", err)
	}
	if err := os.Rename(disk+".saved", disk); err != nil {
		t.Fatal(err)
	}
	if err := client.RecoverBox(ctx, "recover-disk", dto.RecoverBoxDTO{}); err != nil {
		t.Fatalf("retry after repairing missing disk: %v", err)
	}
	result, err = client.Exec(ctx, "recover-disk", "cat", "/root/recovery-marker")
	if err != nil || result.ExitCode != 0 || result.StdOut != "persisted\n" {
		t.Fatalf("failed recovery lost disk changes: result=%v error=%v", result, err)
	}
}
