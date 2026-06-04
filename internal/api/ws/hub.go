package ws

import (
	"net/http"
	"sync"
	"time"

	"github.com/gorilla/websocket"
)

const (
	writeWait  = 10 * time.Second
	pongWait   = 60 * time.Second
	pingPeriod = (pongWait * 9) / 10
)

var upgrader = websocket.Upgrader{
	ReadBufferSize:  1024,
	WriteBufferSize: 1024,
	CheckOrigin: func(r *http.Request) bool {
		return true
	},
}

// Hub broadcasts status snapshots to connected WebSocket clients.
type Hub struct {
	mu       sync.RWMutex
	clients  map[*client]struct{}
	snapshot func() ([]byte, error)
}

func NewHub(snapshot func() ([]byte, error)) *Hub {
	return &Hub{
		clients:  make(map[*client]struct{}),
		snapshot: snapshot,
	}
}

// Publish builds a fresh snapshot and pushes it to all clients.
func (h *Hub) Publish() {
	if h.snapshot == nil {
		return
	}
	data, err := h.snapshot()
	if err != nil || len(data) == 0 {
		return
	}
	h.broadcast(data)
}

func (h *Hub) broadcast(data []byte) {
	h.mu.RLock()
	defer h.mu.RUnlock()
	for c := range h.clients {
		c.trySend(data)
	}
}

// Serve upgrades the connection and streams status updates until disconnect.
func (h *Hub) Serve(w http.ResponseWriter, r *http.Request) {
	conn, err := upgrader.Upgrade(w, r, nil)
	if err != nil {
		return
	}
	c := &client{hub: h, conn: conn, send: make(chan []byte, 8)}
	h.register(c)
	if data, err := h.snapshot(); err == nil && len(data) > 0 {
		c.trySend(data)
	}
	go c.writePump()
	c.readPump()
}

func (h *Hub) register(c *client) {
	h.mu.Lock()
	h.clients[c] = struct{}{}
	h.mu.Unlock()
}

func (h *Hub) unregister(c *client) {
	h.mu.Lock()
	if _, ok := h.clients[c]; ok {
		delete(h.clients, c)
		close(c.send)
	}
	h.mu.Unlock()
}

type client struct {
	hub  *Hub
	conn *websocket.Conn
	send chan []byte
}

func (c *client) trySend(data []byte) {
	select {
	case c.send <- data:
	default:
	}
}

func (c *client) readPump() {
	defer func() {
		c.hub.unregister(c)
		_ = c.conn.Close()
	}()
	c.conn.SetReadLimit(512)
	_ = c.conn.SetReadDeadline(time.Now().Add(pongWait))
	c.conn.SetPongHandler(func(string) error {
		return c.conn.SetReadDeadline(time.Now().Add(pongWait))
	})
	for {
		if _, _, err := c.conn.ReadMessage(); err != nil {
			return
		}
	}
}

func (c *client) writePump() {
	ticker := time.NewTicker(pingPeriod)
	defer func() {
		ticker.Stop()
		_ = c.conn.Close()
	}()
	for {
		select {
		case msg, ok := <-c.send:
			_ = c.conn.SetWriteDeadline(time.Now().Add(writeWait))
			if !ok {
				_ = c.conn.WriteMessage(websocket.CloseMessage, []byte{})
				return
			}
			if err := c.conn.WriteMessage(websocket.TextMessage, msg); err != nil {
				return
			}
		case <-ticker.C:
			_ = c.conn.SetWriteDeadline(time.Now().Add(writeWait))
			if err := c.conn.WriteMessage(websocket.PingMessage, nil); err != nil {
				return
			}
		}
	}
}
