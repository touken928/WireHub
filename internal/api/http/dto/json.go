package dto

// EmptySlice returns a non-nil empty slice so JSON encodes as [] not null.
func EmptySlice[T any]() []T {
	return make([]T, 0)
}
