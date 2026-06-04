package runtime

import "github.com/touken928/wirehub/internal/domain/runtime"

// Callbacks bridges stack lifecycle to control-plane orchestration without importing service.
type Callbacks interface {
	LoadSyncBundle() (runtime.SyncBundle, error)
	OnStarted(dp Dataplane)
	OnStopped()
}
