package boxlite

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	sdk "github.com/boxlite-ai/boxlite/sdks/go"
)

func TestOverlayBDRunnerReachesNativeGate(t *testing.T) {
	t.Chdir(t.TempDir())
	for _, source := range []string{"local", "registry"} {
		cfg := ClientConfig{HomeDir: "relative-home", OverlayBDEnabled: true, OverlayBDSource: source, InsecureRegistries: []string{"127.0.0.1:5000"}}
		if source == "local" {
			cfg.OverlayBDImageDir = "relative-layout"
		}
		client, err := NewClient(context.Background(), cfg)
		if client != nil {
			client.Close()
			t.Fatal("invalid OverlayBD configuration must not fall back to OCI")
		}
		var nativeError *sdk.Error
		if !errors.As(err, &nativeError) ||
			(nativeError.Code != sdk.ErrStorage && nativeError.Code != sdk.ErrUnsupported) ||
			!strings.Contains(err.Error(), "failed to create boxlite runtime") || strings.Contains(err.Error(), "verified TLS") {
			t.Fatalf("expected wrapped native feature/platform/path error for %s, got %v", source, err)
		}
	}
}

func TestOverlayBDSourceContract(t *testing.T) {
	for _, tc := range []struct {
		source, directory, message string
		enabled                    bool
	}{
		{"local", "", "IMAGE_DIR is required", true},
		{"registry", "/layout", "IMAGE_DIR must be empty", true},
		{"typo", "", "SOURCE must be local or registry", true},
		{"registry", "", "registry username", true},
		{"registry", "/unused", "registry username", false},
	} {
		t.Run(tc.source+tc.directory+tc.message, func(t *testing.T) {
			home := filepath.Join(t.TempDir(), "runtime")
			client, err := NewClient(context.Background(), ClientConfig{
				HomeDir: home, OverlayBDEnabled: tc.enabled,
				OverlayBDSource: tc.source, OverlayBDImageDir: tc.directory,
				GhcrUsername: "\xff", GhcrToken: "TEST_ONLY_TOKEN",
			})
			if client != nil {
				client.Close()
				t.Fatal("invalid configuration accepted")
			}
			if err == nil || !strings.Contains(err.Error(), tc.message) {
				t.Fatalf("expected %q, got %v", tc.message, err)
			}
			if _, err := os.Stat(home); !os.IsNotExist(err) {
				t.Fatal("invalid configuration created runtime state")
			}
		})
	}
}

func TestOverlayBDDoesNotFallBackToOCI(t *testing.T) {
	home := filepath.Join(t.TempDir(), "runtime")
	client, err := NewClient(context.Background(), ClientConfig{
		HomeDir:          home,
		OverlayBDEnabled: true,
	})
	if client != nil {
		_ = client.Close()
		t.Fatal("OverlayBD must not silently create an OCI runtime")
	}
	if err == nil || !strings.Contains(err.Error(), "BOXLITE_OVERLAYBD_IMAGE_DIR is required") {
		t.Fatal("expected an explicit missing OverlayBD image directory error")
	}
	if _, err := os.Stat(home); !os.IsNotExist(err) {
		t.Fatal("invalid configuration must fail before creating the runtime home")
	}
}

// Operator acceptance: public metadata over explicit HTTP, independently authenticated daemon layers.
func TestOverlayBDRegistryRunnerSmoke(t *testing.T) {
	image := os.Getenv("OVERLAYBD_RUNNER_TEST_IMAGE")
	if image == "" {
		t.Skip("real Linux registry fixture is not configured")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	client, err := NewClient(ctx, ClientConfig{
		HomeDir: t.TempDir(), OverlayBDEnabled: true, OverlayBDSource: "registry",
		InsecureRegistries: []string{strings.SplitN(image, "/", 2)[0]},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	box, err := client.runtime.Create(ctx, image, sdk.WithAutoRemove(false), sdk.WithEntrypoint("/bin/sh"), sdk.WithCmd("-c", "sleep 600"))
	if err != nil {
		t.Fatal(err)
	}
	defer box.Close()
	defer func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		if err := client.runtime.ForceRemove(cleanup, box.ID()); err != nil {
			t.Error(err)
		}
	}()
	if err := box.Start(ctx); err != nil {
		t.Fatal(err)
	}
	if err := box.Stop(ctx); err != nil {
		t.Fatal(err)
	}
}
