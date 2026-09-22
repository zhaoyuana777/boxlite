package boxlite

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	sdk "github.com/boxlite-ai/boxlite/sdks/go"
)

func TestOverlayBDRunnerReachesNativeGate(t *testing.T) {
	client, err := NewClient(context.Background(), ClientConfig{
		HomeDir:           t.TempDir(),
		OverlayBDEnabled:  true,
		OverlayBDImageDir: "relative-layout",
	})
	if client != nil {
		_ = client.Close()
		t.Fatal("invalid OverlayBD directory must not fall back to OCI")
	}
	var nativeError *sdk.Error
	if !errors.As(err, &nativeError) ||
		(nativeError.Code != sdk.ErrStorage && nativeError.Code != sdk.ErrUnsupported) ||
		!strings.Contains(err.Error(), "failed to create boxlite runtime") {
		t.Fatalf("expected wrapped native feature/platform/path error, got %v", err)
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
