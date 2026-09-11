// SPDX-License-Identifier: AGPL-3.0-only
// Copyright (c) 2024 BoxLite AI (originally Daytona Platforms Inc.
// Modified and rebranded for BoxLite

package boxlite

import (
	"context"
	"fmt"
	"os"

	boxlite "github.com/boxlite-ai/boxlite/sdks/go"
	"github.com/boxlite-ai/runner/pkg/api/dto"
	"github.com/containerd/errdefs"
)

// RecoverBox restarts the original VM using its persisted configuration and disks.
// Legacy VM settings are ignored; the saved control-plane volume list only
// validates the local mount record and never changes the VM's configuration.
func (c *Client) RecoverBox(ctx context.Context, boxId string, legacy dto.RecoverBoxDTO) error {
	release, err := c.operations.acquire(ctx, boxId)
	if err != nil {
		return err
	}
	defer release()

	c.logger.Info("recover box", "box", boxId)

	bx, err := c.getOrFetchBox(ctx, boxId)
	if err != nil {
		return fmt.Errorf("recover: locate original box: %w", err)
	}
	info, err := bx.Info(ctx)
	if err != nil {
		return fmt.Errorf("recover: inspect original box: %w", err)
	}
	if info.AutoDelete != 0 {
		return fmt.Errorf("recover: auto-delete box cannot be restarted without losing its disk")
	}
	switch info.State {
	case boxlite.StateRunning, boxlite.StateStopped, boxlite.StateFailed:
	default:
		return fmt.Errorf("recover: original box is in unsupported state %s", info.State)
	}
	if err := bx.Stop(ctx); err != nil {
		return fmt.Errorf("recover: stop original VM: %w", err)
	}
	// Stop consumes the runtime handle. Fetch by immutable ID, never GetOrCreate.
	c.evictBox(boxId, bx)
	bx, err = c.runtime.Get(ctx, info.ID)
	if err != nil {
		return fmt.Errorf("recover: reload original box: %w", err)
	}
	c.mu.Lock()
	c.boxes[boxId] = bx
	c.mu.Unlock()
	if err := c.restoreVolumeMounts(ctx, boxId, legacy.Volumes); err != nil {
		if !os.IsNotExist(err) || len(legacy.Volumes) > 0 {
			return fmt.Errorf("recover: restore original volume mounts: %w", err)
		}
	}
	if err := bx.Start(ctx); err != nil {
		return fmt.Errorf("recover: start original VM: %w", err)
	}
	return nil
}

// UpdateNetworkSettings updates the network allowlist/blocklist for a box.
// TODO: Implement when BoxLite Go SDK exposes network configuration.
func (c *Client) UpdateNetworkSettings(ctx context.Context, boxId string, settings dto.UpdateNetworkSettingsDTO) error {
	c.logger.Warn("update network settings not yet implemented in BoxLite", "box", boxId)
	return errdefs.ErrNotImplemented.WithMessage("live network settings update is not supported by the BoxLite Go SDK")
}

// GetDaemonVersion returns the runtime version for the legacy API field.
func (c *Client) GetDaemonVersion(ctx context.Context, boxId string) (string, error) {
	return "boxlite", nil
}
