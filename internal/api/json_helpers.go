package api

// emptySlice returns a non-nil empty slice so JSON encodes as [] not null.
func emptySlice[T any]() []T {
	return make([]T, 0)
}
