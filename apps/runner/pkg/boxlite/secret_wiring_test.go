// Copyright 2026 BoxLite AI
// SPDX-License-Identifier: AGPL-3.0-only

package boxlite

import (
	"go/parser"
	"go/token"
	"testing"
)

// TestCreateAppliesSecrets guards the hop that `secretSpecs` alone cannot: a
// pure mapping function test passes whether or not Client.Create actually feeds
// the resulting Secrets into the SDK. A behavioral test cannot see this hop —
// boxlite.BoxOption closes over an unexported config (sdks/go/options.go), so a
// fake runtime receives opaque functions it cannot inspect. The AST source
// check is the strongest available guard, the same technique the repo already
// uses for the Start coupling (TestCreateHasNoFallibleStepAfterStart).
func TestCreateAppliesSecrets(t *testing.T) {
	fileSet := token.NewFileSet()
	parsed, err := parser.ParseFile(fileSet, "client.go", nil, 0)
	if err != nil {
		t.Fatalf("parse client.go: %v", err)
	}

	create := findMethod(parsed, "Client", "create")
	if create == nil {
		t.Fatal("Client.create not found in client.go")
	}

	if findCall(create.Body, "boxlite", "WithSecret") == nil {
		t.Fatal("Client.Create no longer calls boxlite.WithSecret; secrets would be dropped")
	}
}
