package com.example.model

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.filter

data class Product(val id: Long, val name: String, val price: Double, val inStock: Boolean)

interface ProductRepository {
    fun findAll(): Flow<Product>
    suspend fun findById(id: Long): Product?
    suspend fun save(product: Product): Product
}

class ProductService(private val repository: ProductRepository) {

    fun listAvailable(): Flow<Product> =
        repository.findAll().filter { it.inStock }

    suspend fun get(id: Long): Product =
        repository.findById(id) ?: throw NoSuchElementException("Product $id not found")

    suspend fun updatePrice(id: Long, price: Double): Product {
        val product = get(id)
        return repository.save(product.copy(price = price))
    }

    suspend fun markOutOfStock(id: Long) {
        val product = get(id)
        repository.save(product.copy(inStock = false))
    }
}

fun Double.formatPrice(): String = "%.2f".format(this)
