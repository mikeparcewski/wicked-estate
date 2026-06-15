package com.example

import scala.concurrent.{ExecutionContext, Future}

case class Item(id: Long, name: String, quantity: Int)

trait ItemRepository {
  def findAll(): Future[Seq[Item]]
  def findById(id: Long): Future[Option[Item]]
  def save(item: Item): Future[Item]
}

class InventoryService(repo: ItemRepository)(implicit ec: ExecutionContext) {

  def listAll(): Future[Seq[Item]] = repo.findAll()

  def get(id: Long): Future[Item] =
    repo.findById(id).map(_.getOrElse(throw new NoSuchElementException(s"Item $id")))

  def adjustQuantity(id: Long, delta: Int): Future[Item] =
    get(id).flatMap(item => repo.save(item.copy(quantity = item.quantity + delta)))

  def remove(id: Long): Future[Item] =
    get(id).flatMap(item => repo.save(item.copy(quantity = 0)))
}

object InventoryService {
  def apply(repo: ItemRepository)(implicit ec: ExecutionContext): InventoryService =
    new InventoryService(repo)
}
