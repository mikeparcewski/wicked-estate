package shop

trait Priced {
  def price(): Int
}

abstract class Item {
  def label(): String = "item"
}

class Book extends Item with Priced {
  override def label(): String = "book"
  def price(): Int = 10
}

object Shop {
  def stock(): Book = new Book()
  def total(): Int = stock().price()
}
