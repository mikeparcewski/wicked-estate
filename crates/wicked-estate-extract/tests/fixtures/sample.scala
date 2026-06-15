package com.example.pipeline

import scala.collection.mutable.ListBuffer

case class Record(id: Int, payload: String, weight: Double)

object RecordOps {
  val MaxWeight: Double = 1000.0

  def validate(record: Record): Boolean = record.weight <= MaxWeight && record.payload.nonEmpty

  def normalize(record: Record): Record = record.copy(weight = record.weight / MaxWeight)

  def describe(record: Record): String = record match {
    case Record(id, _, w) if w > 0.5 => s"heavy record $id"
    case Record(id, _, _)             => s"light record $id"
  }
}

class Pipeline(val name: String) {
  private val buffer = ListBuffer.empty[Record]

  def ingest(record: Record): Unit = {
    if (RecordOps.validate(record)) {
      buffer += RecordOps.normalize(record)
    }
  }

  def flush(): List[Record] = {
    val result = buffer.toList
    buffer.clear()
    result
  }

  def report(): String = {
    val descriptions = buffer.map(RecordOps.describe).mkString(", ")
    s"Pipeline[$name]: $descriptions"
  }
}
