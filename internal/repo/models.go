package repo

import "time"

type Admin struct {
	ID           uint   `gorm:"primaryKey" json:"id"`
	Username     string `gorm:"uniqueIndex;not null" json:"username"`
	PasswordHash string `gorm:"not null" json:"-"`
}

type Settings struct {
	ID               uint   `gorm:"primaryKey" json:"id"`
	ServerPublicKey  string `json:"server_public_key"`
	ServerPrivateKey string `json:"-"`
	Endpoint         string `json:"endpoint"`
	ListenPort       int    `json:"listen_port"`
	WGSubnet         string `json:"wg_subnet"`
	HubIP            string `json:"hub_ip"`
	DNSIP            string `json:"dns_ip"`
	DNSSuffix        string `json:"dns_suffix"`
	MTU              int      `json:"mtu"`
	StatusInterval   int      `json:"status_interval"`
	UpstreamDNS      []string `gorm:"serializer:json" json:"upstream_dns"`
}

type Peer struct {
	ID            uint      `gorm:"primaryKey" json:"id"`
	Name          string    `gorm:"uniqueIndex;not null" json:"name"`
	PublicKey     string    `gorm:"uniqueIndex;not null" json:"public_key"`
	PrivateKey    string    `gorm:"not null" json:"-"`
	WGIP          string    `gorm:"uniqueIndex;not null" json:"wg_ip"`
	GroupID       uint      `gorm:"index;not null" json:"group_id"`
	Enabled       bool      `gorm:"default:true" json:"enabled"`
	DNSName       string    `json:"dns_name"`
	LastHandshake int64     `json:"last_handshake"`
	RxBytes       int64     `json:"rx_bytes"`
	TxBytes       int64     `json:"tx_bytes"`
	CreatedAt     time.Time `json:"created_at"`
}

type PeerGroup struct {
	ID   uint    `gorm:"primaryKey" json:"id"`
	Name string  `gorm:"uniqueIndex;not null" json:"name"`
	PosX float64 `json:"pos_x"`
	PosY float64 `json:"pos_y"`
}

// GroupLink is stored with FromGroupID < ToGroupID (undirected access).
type GroupLink struct {
	ID          uint `gorm:"primaryKey" json:"id"`
	FromGroupID uint `gorm:"uniqueIndex:idx_group_link_pair;not null" json:"from_group_id"`
	ToGroupID   uint `gorm:"uniqueIndex:idx_group_link_pair;not null" json:"to_group_id"`
}

type DNSRecord struct {
	ID       uint   `gorm:"primaryKey" json:"id"`
	Hostname string `gorm:"uniqueIndex;not null" json:"hostname"`
	IP       string `gorm:"not null" json:"ip"`
	PeerID   *uint  `json:"peer_id"`
	Manual   bool   `json:"manual"`
}
