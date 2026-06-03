package l4

import (
	"errors"
	"testing"
)

func TestEphemeralPortPoolSkipsReserved(t *testing.T) {
	pool := newEphemeralPortPool()
	pool.reserveHubPorts([]int{20001, 20002, 20003})

	for i := 0; i < 512; i++ {
		port, err := pool.pick()
		if err != nil {
			t.Fatal(err)
		}
		switch port {
		case 20001, 20002, 20003:
			t.Fatalf("picked reserved port %d", port)
		}
	}
}

func TestEphemeralPortPoolSkipsInUse(t *testing.T) {
	pool := newEphemeralPortPool()
	first, err := pool.pick()
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 512; i++ {
		port, err := pool.pick()
		if err != nil {
			t.Fatal(err)
		}
		if port == first {
			t.Fatalf("reused in-use port %d", first)
		}
	}
}

func TestEphemeralPortPoolResetInUse(t *testing.T) {
	pool := newEphemeralPortPool()
	const n = 32
	for i := 0; i < n; i++ {
		if _, err := pool.pick(); err != nil {
			t.Fatalf("initial pick %d: %v", i, err)
		}
	}
	pool.resetInUse()
	for i := 0; i < n; i++ {
		if _, err := pool.pick(); err != nil {
			t.Fatalf("pick %d after resetInUse: %v", i, err)
		}
	}
}

func TestEphemeralPortPoolExhausted(t *testing.T) {
	pool := newEphemeralPortPool()
	reserved := make([]int, 0, EphemeralPortMax-EphemeralPortMin+1)
	for port := EphemeralPortMin; port <= EphemeralPortMax; port++ {
		reserved = append(reserved, port)
	}
	pool.reserveHubPorts(reserved)

	_, err := pool.pick()
	if !errors.Is(err, errPortsExhausted{}) {
		t.Fatalf("pick() err = %v, want errPortsExhausted", err)
	}
}
