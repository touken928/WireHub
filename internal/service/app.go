package service

import (
	domainruntime "github.com/touken928/wirehub/internal/domain/runtime"
	"github.com/touken928/wirehub/internal/repo"
	vpnruntime "github.com/touken928/wirehub/internal/vpn/runtime"
)

// App is the control-plane application root (persistence + network hub).
type App struct {
	Store  *repo.Store
	Hub    *Hub
	Status *StatusService
}

// NewApp wires the hub and status service to this application instance.
func NewApp(st *repo.Store) *App {
	a := &App{Store: st}
	a.Hub = NewHub(a)
	a.Status = newStatusService(a)
	a.Hub.SetStatusPublisher(a.Status)
	return a
}

// LoadSyncBundle implements runtime.Callbacks.
func (a *App) LoadSyncBundle() (domainruntime.SyncBundle, error) {
	return a.loadSyncBundle()
}

// OnStarted implements vpnruntime.Callbacks.
func (a *App) OnStarted(dp vpnruntime.Dataplane) {
	a.Hub.onStarted(dp)
}

// OnStopped implements runtime.Callbacks.
func (a *App) OnStopped() {
	a.Hub.onStopped()
}
