extends Node

func clamp_speed(v: float) -> float:
	return clamp(v, 0.0, MAX_SPEED)
