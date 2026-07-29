package main

import (
	"fmt"

	"example.com/semblock-go-fixture/internal/orders"
	"example.com/semblock-go-fixture/internal/users"
)

func main() {
	fmt.Println(len(users.FindByID()), len(orders.Queries()))
}
