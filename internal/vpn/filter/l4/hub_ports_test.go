package l4

import (
	"slices"
	"testing"
)

func TestReservedHubPorts(t *testing.T) {
	tests := []struct {
		name     string
		webPort  int
		forwards []ForwardRule
		want     []int
	}{
		{
			name:    "system listen ports only",
			webPort: 8443,
			want:    []int{53, 8443},
		},
		{
			name:    "includes enabled forward listen ports",
			webPort: 8443,
			forwards: []ForwardRule{
				{ListenPort: 9000, Enabled: true},
				{ListenPort: 9001, Enabled: false},
			},
			want: []int{53, 8443, 9000},
		},
		{
			name:    "deduplicates when forward matches web port",
			webPort: 9000,
			forwards: []ForwardRule{
				{ListenPort: 9000, Enabled: true},
			},
			want: []int{53, 9000},
		},
		{
			name:    "ignores invalid port numbers",
			webPort: 8443,
			forwards: []ForwardRule{
				{ListenPort: 0, Enabled: true},
				{ListenPort: 70000, Enabled: true},
				{ListenPort: 9100, Enabled: true},
			},
			want: []int{53, 8443, 9100},
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := ReservedHubPorts(tc.webPort, tc.forwards)
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
