package ingress

import (
	"slices"
	"testing"
)

func TestReservedHubPorts(t *testing.T) {
	tests := []struct {
		name      string
		tunnelWeb int
		forwards  []ForwardRule
		want      []int
	}{
		{
			name:      "system listen ports only",
			tunnelWeb: HubTunnelWebPort,
			want:      []int{53, 80},
		},
		{
			name:      "includes forward listen ports",
			tunnelWeb: HubTunnelWebPort,
			forwards: []ForwardRule{
				{ListenPort: 9000},
				{ListenPort: 9001},
			},
			want: []int{53, 80, 9000, 9001},
		},
		{
			name:      "deduplicates when forward matches tunnel web port",
			tunnelWeb: 9000,
			forwards: []ForwardRule{
				{ListenPort: 9000},
			},
			want: []int{53, 9000},
		},
		{
			name:      "ignores invalid port numbers",
			tunnelWeb: HubTunnelWebPort,
			forwards: []ForwardRule{
				{ListenPort: 0},
				{ListenPort: 70000},
				{ListenPort: 9100},
			},
			want: []int{53, 80, 9100},
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := ReservedHubPorts(tc.tunnelWeb, tc.forwards)
			if !slices.Equal(got, tc.want) {
				t.Fatalf("ReservedHubPorts() = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestReservedHubPortsAlwaysIncludesDNS(t *testing.T) {
	got := ReservedHubPorts(18080, nil)
	if !slices.Contains(got, HubDNSPort) {
		t.Fatalf("missing hub DNS port %d in %v", HubDNSPort, got)
	}
}
