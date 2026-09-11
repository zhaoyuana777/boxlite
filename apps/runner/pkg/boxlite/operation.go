// Copyright 2026 BoxLite AI
// SPDX-License-Identifier: AGPL-3.0

package boxlite

import (
	"context"
	"sync"
)

// boxOperations coordinates local lifecycle calls, not whole migration jobs:
// uploading or deleting an archive must not prevent a user starting the box.
type boxOperations struct {
	mu     sync.Mutex
	active map[string]chan struct{}
}

func (o *boxOperations) acquire(ctx context.Context, boxID string) (func(), error) {
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		o.mu.Lock()
		done, busy := o.active[boxID]
		if !busy {
			if o.active == nil {
				o.active = make(map[string]chan struct{})
			}
			done = make(chan struct{})
			o.active[boxID] = done
		}
		o.mu.Unlock()
		if !busy {
			return func() {
				o.mu.Lock()
				delete(o.active, boxID)
				close(done)
				o.mu.Unlock()
			}, nil
		}
		select {
		case <-ctx.Done():
			return nil, ctx.Err()
		case <-done:
		}
	}
}
