package main

import (
	"log"

	"github.com/touken928/wirehub/internal/bootstrap"
	"github.com/touken928/wirehub/internal/config"
)

func main() {
	cfg, err := config.ParseFlags()
	if err != nil {
		log.Fatalf("config: %v", err)
	}
	if err := bootstrap.Run(cfg); err != nil {
		log.Fatalf("wirehub: %v", err)
	}
}
