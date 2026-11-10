object Parser {
  def parseOr(s: String, fallback: Int): Int =
    try s.toInt
    catch { case _: NumberFormatException => fallback }
}
