package repo

import "testing"

func TestUpstreamDNSResolversEmpty(t *testing.T) {
	s := &Settings{DNSIP: "100.127.0.1"}
	if got := s.UpstreamDNSResolvers(); got != nil {
		t.Fatalf("UpstreamDNSResolvers() = %v, want nil", got)
	}
}

func TestClientDNSOnlyHub(t *testing.T) {
	s := &Settings{
		DNSIP:       "100.127.0.1",
		UpstreamDNS: []string{"8.8.8.8", "1.1.1.1"},
	}
	got := s.ClientDNS()
	if len(got) != 1 || got[0] != "100.127.0.1" {
		t.Fatalf("ClientDNS() = %v, want only hub DNS IP", got)
	}
}
