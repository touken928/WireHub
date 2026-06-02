package service

// SetDNSUpstream updates upstream resolvers on the live DNS server when the stack is running.
func (h *Hub) SetDNSUpstream(upstream []string) {
	if dns, err := h.dnsServer(); err == nil {
		dns.SetUpstream(upstream)
	}
}
