package vpn

import (
	"net/http"

	"github.com/touken928/wirehub/internal/config"
	domainruntime "github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/vpn/runtime"
)

// Stack is the VPN data-plane runtime.
type Stack = runtime.Stack

// NewStack wires control-plane callbacks into the VPN runtime.
func NewStack(cfg *config.RuntimeConfig, cb runtime.Callbacks, handler http.Handler) *Stack {
	return runtime.NewStack(cfg, cb, handler)
}

// Dataplane is the runtime dataplane API.
type Dataplane = runtime.Dataplane

// SyncBundle is the portable runtime snapshot.
type SyncBundle = domainruntime.SyncBundle
