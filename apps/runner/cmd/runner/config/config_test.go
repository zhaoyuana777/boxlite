package config

import (
	"testing"

	"github.com/go-playground/validator/v10"
)

func TestTunnelCapacityValidation(t *testing.T) {
	validate := validator.New()
	for _, tc := range []struct {
		total, perBox int
		valid         bool
	}{
		{256, 32, true}, {1, 1, true}, {0, 1, false},
		{-1, 1, false}, {1, 0, false}, {1, -1, false}, {1, 2, false},
	} {
		cfg := &Config{MaxTunnels: tc.total, MaxTunnelsPerBox: tc.perBox}
		err := validate.StructPartial(cfg, "MaxTunnels", "MaxTunnelsPerBox")
		if (err == nil) != tc.valid {
			t.Errorf("total=%d perBox=%d: valid=%v, want %v", tc.total, tc.perBox, err == nil, tc.valid)
		}
	}
}
