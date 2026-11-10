package store

func Save(path string, data []byte) error {
	if err := write(path, data); err != nil {
		return fmt.Errorf("save %s: %w", path, err)
	}
	return nil
}

func Load(path string) (*Store, error) {
	data, err := readFile(path)
	if err != nil {
		return nil, err
	}
	return decode(data, path), nil
}
