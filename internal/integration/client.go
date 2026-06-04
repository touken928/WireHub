package integration

import (
	"fmt"
	"net/netip"
	"testing"
	"time"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/repo"
	"github.com/touken928/wirehub/internal/vpn/tunnel"
	"golang.zx2c4.com/wireguard/conn"
	"golang.zx2c4.com/wireguard/device"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

func startPeerClient(hubIP, hubPubKey string, listenPort int, peer repo.Peer) (*device.Device, *netstack.Net, error) {
	clientTun, clientNet, err := netstack.CreateNetTUN(
		[]netip.Addr{netip.MustParseAddr(peer.WGIP)},
		[]netip.Addr{netip.MustParseAddr(hubIP)},
		1420,
	)
	if err != nil {
		return nil, nil, err
	}

	clientDev := device.NewDevice(clientTun, conn.NewDefaultBind(), device.NewLogger(device.LogLevelError, ""))
	peerPrivHex, err := tunnel.KeyToHex(peer.PrivateKey)
	if err != nil {
		return nil, nil, err
	}
	hubPubHex, err := tunnel.KeyToHex(hubPubKey)
	if err != nil {
		return nil, nil, err
	}
	cfg := fmt.Sprintf(`private_key=%s
public_key=%s
endpoint=127.0.0.1:%d
allowed_ip=%s
persistent_keepalive_interval=1
`, peerPrivHex, hubPubHex, listenPort, config.DefaultSubnet)
	if err := clientDev.IpcSet(cfg); err != nil {
		return nil, nil, err
	}
	if err := clientDev.Up(); err != nil {
		return nil, nil, err
	}
	return clientDev, clientNet, nil
}

func waitHandshake(t *testing.T, mgr *tunnel.Manager, peerPubKey string, timeout time.Duration) error {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		stats, err := mgr.GetStats()
		if err != nil {
			return err
		}
		if s, ok := stats[peerPubKey]; ok && !s.LastHandshake.IsZero() {
			return nil
		}
		time.Sleep(50 * time.Millisecond)
	}
	return fmt.Errorf("wireguard handshake timeout")
}
