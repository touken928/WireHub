package peer

import (
	"testing"
	"time"
)

func TestPeerIsOnline(t *testing.T) {
	now := time.Unix(1_000_000, 0)
	hs := now.Add(-2 * time.Minute).Unix()

	if !PeerIsOnline(true, hs, now) {
		t.Fatal("handshake within window should be online")
	}
	if PeerIsOnline(true, now.Add(-4*time.Minute).Unix(), now) {
		t.Fatal("handshake beyond window should be offline")
	}
	if PeerIsOnline(false, hs, now) {
		t.Fatal("disabled peer must not be online")
	}
	if PeerIsOnline(true, 0, now) {
		t.Fatal("never handshaked must be offline")
	}
}
