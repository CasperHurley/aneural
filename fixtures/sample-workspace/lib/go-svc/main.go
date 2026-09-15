package main

import (
	"fmt"

	"example.com/gosvc/internal/handler"
	"github.com/gorilla/mux"
)

// NOTE: wire graceful shutdown
func main() {
	fmt.Println(handler.Name(), mux.NewRouter())
}
