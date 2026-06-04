package domain

import (
	"strings"
	"testing"

	"github.com/touken928/wirehub/internal/config"
	"github.com/touken928/wirehub/internal/vpn/filter/l4"
)

func TestValidateForwardTargetHost(t *testing.T) {
	tests := []struct {
		in      string
		want    string
		wantErr bool
	}{
		{"10.0.0.2", "10.0.0.2", false},
		{"app.wirehub", "app.wirehub", false},
		{"hub.wirehub", "hub.wirehub", false},
		{"service.example.com", "service.example.com", false},
		{"app", "", true},
		{"", "", true},
	}
	for _, tc := range tests {
		got, err := ValidateForwardTargetHost(tc.in)
		if tc.wantErr {
			if err == nil {
				t.Errorf("%q: want error", tc.in)
			}
			continue
		}
		if err != nil {
			t.Errorf("%q: %v", tc.in, err)
			continue
		}
		if got != tc.want {
			t.Errorf("%q: got %q want %q", tc.in, got, tc.want)
		}
	}
}

func TestValidateForwardListenPortReservesSystemListen(t *testing.T) {
	tunnelWeb := l4.HubTunnelWebPort

	tests := []struct {
		name     string
		port     int
		protocol string
		wantErr  string
	}{
		{
			name:     "dns port",
			port:     l4.HubDNSPort,
			protocol: ForwardProtoUDP,
			wantErr:  "reserved for hub DNS",
		},
		{
			name:     "web tcp",
			port:     tunnelWeb,
			protocol: ForwardProtoTCP,
			wantErr:  "hub web UI and API",
		},
		{
			name:     "allowed wireguard host port on hub udp forward",
			port:     config.DefaultPort,
			protocol: ForwardProtoUDP,
		},
		{
			name:     "allowed custom port",
			port:     9000,
			protocol: ForwardProtoTCP,
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := ValidateForwardListenPort(tc.port, tunnelWeb, tc.protocol)
			if tc.wantErr == "" {
				if err != nil {
					t.Fatalf("unexpected error: %v", err)
				}
				return
			}
			if err == nil || !strings.Contains(err.Error(), tc.wantErr) {
				t.Fatalf("err = %v, want substring %q", err, tc.wantErr)
			}
		})
	}
}

func TestValidateForwardProtocol(t *testing.T) {
	for _, proto := range []string{"tcp", "TCP", " udp "} {
		got, err := ValidateForwardProtocol(proto)
		if err != nil {
			t.Fatalf("%q: %v", proto, err)
		}
		if got != strings.ToLower(strings.TrimSpace(proto)) {
			t.Fatalf("got %q", got)
		}
	}
	if _, err := ValidateForwardProtocol("icmp"); err == nil {
		t.Fatal("expected error for unsupported protocol")
	}
}
