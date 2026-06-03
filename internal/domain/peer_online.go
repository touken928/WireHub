package domain

import "time"

// PeerOnlineWindow is how recently a peer must have handshaked to count as online.
const PeerOnlineWindow = 3 * time.Minute

// PeerIsOnline reports whether an enabled peer is considered connected.
func PeerIsOnline(enabled bool, lastHandshakeUnix int64, now time.Time) bool {
	if !enabled || lastHandshakeUnix <= 0 {
		return false
	}
	return now.Sub(time.Unix(lastHandshakeUnix, 0)) < PeerOnlineWindow
}
