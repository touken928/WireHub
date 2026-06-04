package runtime

import (
	"net/netip"

	"github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/vpn/ingress"
)

func ingressForwardRules(rules []runtime.ForwardRule) []ingress.ForwardRule {
	out := make([]ingress.ForwardRule, 0, len(rules))
	for _, r := range rules {
		out = append(out, ingress.ForwardRule{
			ID:         r.ID,
			ListenPort: r.ListenPort,
			Protocol:   r.Protocol,
			TargetHost: r.TargetHost,
			TargetPort: r.TargetPort,
		})
	}
	return out
}

func ingressMapRules(rules []runtime.MapRule) []ingress.MapRule {
	out := make([]ingress.MapRule, 0, len(rules))
	for _, r := range rules {
		vip, err := netip.ParseAddr(r.VirtualIP)
		if err != nil {
			continue
		}
		out = append(out, ingress.MapRule{
			ID:              r.ID,
			Slug:            r.Slug,
			TargetHost:      r.TargetHost,
			VirtualIP:       vip,
			AllowedGroupIDs: r.AllowedGroupIDs,
		})
	}
	return out
}

func parseMapVIPs(rules []runtime.MapRule) []netip.Addr {
	out := make([]netip.Addr, 0, len(rules))
	for _, r := range rules {
		vip, err := netip.ParseAddr(r.VirtualIP)
		if err != nil || !vip.IsValid() {
			continue
		}
		out = append(out, vip)
	}
	return out
}
