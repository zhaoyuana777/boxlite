package controllers

import "sync"

// One instance belongs to the registered handler, shared by all Proxy connections.
type tunnelLimits struct {
	slots  chan struct{}
	mu     sync.Mutex
	perBox int
	boxes  map[string]int
}

func newTunnelLimits(total, perBox int) *tunnelLimits {
	return &tunnelLimits{slots: make(chan struct{}, total), perBox: perBox, boxes: make(map[string]int)}
}

func (l *tunnelLimits) acquireBox(id string) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.boxes[id] >= l.perBox {
		return false
	}
	l.boxes[id]++
	return true
}

func (l *tunnelLimits) releaseBox(id string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.boxes[id]--
	if l.boxes[id] == 0 {
		delete(l.boxes, id)
	}
}
