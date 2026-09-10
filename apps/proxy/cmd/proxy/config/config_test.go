// Copyright 2025 BoxLite AI (originally Daytona Platforms Inc.
// Modified by BoxLite AI, 2025-2026
// SPDX-License-Identifier: AGPL-3.0

package config

import (
	"maps"
	"testing"

	"github.com/go-playground/validator/v10"
)

func TestTunnelCapacityValidation(t *testing.T) {
	validate := validator.New()
	for _, limit := range []int{-1, 0, 1, 256} {
		err := validate.StructPartial(&Config{MaxTunnels: limit}, "MaxTunnels")
		if (err == nil) != (limit > 0) {
			t.Errorf("limit=%d: valid=%v", limit, err == nil)
		}
	}
}

func TestGetOtelHeaders(t *testing.T) {
	cases := []struct {
		name    string
		headers string
		want    map[string]string
	}{
		{"empty", "", map[string]string{}},
		{"single pair", "authorization=Bearer abc", map[string]string{"authorization": "Bearer abc"}},
		{"multiple pairs", "a=1,b=2", map[string]string{"a": "1", "b": "2"}},
		{"whitespace trimmed", " a = 1 , b = 2 ", map[string]string{"a": "1", "b": "2"}},
		{"malformed pair skipped", "a=1,broken,b=2", map[string]string{"a": "1", "b": "2"}},
		{"value keeps extra equals", "a=x=y", map[string]string{"a": "x=y"}},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			c := &Config{OtelHeaders: tc.headers}
			got := c.GetOtelHeaders()
			if !maps.Equal(got, tc.want) {
				t.Errorf("GetOtelHeaders(%q) = %v, want %v", tc.headers, got, tc.want)
			}
		})
	}
}
