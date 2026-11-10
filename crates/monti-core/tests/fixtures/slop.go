package store

func Save(path string, data []byte) {
	err := write(path, data)
	if err != nil {
	}
}

func Read(path string) ([]byte, error) {
	return readFile(path)
}

func New() *Store {
	s := &Store{}
	if s != nil {
		s.init()
	}
	return s
}

func Bump(count int) int {
	// increment count
	count++
	return count
}
