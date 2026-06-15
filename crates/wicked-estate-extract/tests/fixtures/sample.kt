package com.example.analytics

import kotlin.math.sqrt
import kotlin.math.abs

data class Metric(val name: String, val value: Double, val unit: String)

class MetricProcessor(private val threshold: Double) {

    private val results = mutableListOf<Metric>()

    fun process(metric: Metric): Double {
        val normalized = normalize(metric.value)
        results.add(metric.copy(value = normalized))
        return normalized
    }

    private fun normalize(value: Double): Double {
        return if (abs(value) > threshold) threshold else value
    }

    fun summarize(): String {
        val avg = results.map { it.value }.average()
        return "count=${results.size} avg=$avg"
    }

    companion object {
        const val DEFAULT_THRESHOLD = 100.0

        fun create(): MetricProcessor = MetricProcessor(DEFAULT_THRESHOLD)
    }
}

fun Metric.isOutlier(limit: Double): Boolean = abs(value) > limit

fun runPipeline(metrics: List<Metric>): String {
    val processor = MetricProcessor.create()
    metrics.forEach { processor.process(it) }
    return processor.summarize()
}
