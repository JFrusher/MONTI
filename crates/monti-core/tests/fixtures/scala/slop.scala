object Store {
  def load(): Unit = {
    try { parse() } catch { case e: Exception => }
    try { parse() } catch { case e: Exception => /* ignore */ }
  }

  def bump(counter: Counter): Unit = {
    // increment counter
    counter.increment()
  }
}
