package filter

import "testing"

func TestBuildClaimedListenPorts(t *testing.T) {
	tcp, udp := buildClaimedListenPorts(8443, []PortForwardRule{
		{ListenPort: 8080, Protocol: "tcp", Enabled: true},
		{ListenPort: 8080, Protocol: "udp", Enabled: false},
		{ListenPort: 9000, Protocol: "udp", Enabled: true},
	})
	if _, ok := tcp[53]; !ok {
		t.Fatal("tcp 53 reserved")
	}
	if _, ok := tcp[8443]; !ok {
		t.Fatal("tcp hub port reserved")
	}
	if _, ok := tcp[8080]; !ok {
		t.Fatal("tcp 8080 claimed by enabled forward")
	}
	if _, ok := tcp[9000]; ok {
		t.Fatal("tcp 9000 should not be claimed")
	}
	if _, ok := udp[8080]; ok {
		t.Fatal("udp 8080 disabled forward should not claim")
	}
	if _, ok := udp[9000]; !ok {
		t.Fatal("udp 9000 claimed")
	}
}
