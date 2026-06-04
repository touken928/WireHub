package ingress

import (
	"errors"
	"fmt"
	"log"
	"net/http"
	"net/netip"

	"golang.zx2c4.com/wireguard/tun/netstack"
)

// StartWebServer serves the admin UI/API on hubIP:port inside the netstack (system listen).
func StartWebServer(tnet *netstack.Net, hubIP string, port int, handler http.Handler) (*http.Server, error) {
	addr, err := netip.ParseAddr(hubIP)
	if err != nil {
		return nil, fmt.Errorf("parse hub ip: %w", err)
	}
	ln, err := tnet.ListenTCPAddrPort(netip.AddrPortFrom(addr, uint16(port)))
	if err != nil {
		return nil, fmt.Errorf("listen %s:%d on netstack: %w", hubIP, port, err)
	}
	log.Printf("WireHub tunnel web: http://%s:%d (netstack)", hubIP, port)
	srv := &http.Server{Handler: handler}
	go func() {
		if err := srv.Serve(ln); err != nil && !errors.Is(err, http.ErrServerClosed) {
			log.Printf("tunnel web server: %v", err)
		}
	}()
	return srv, nil
}
