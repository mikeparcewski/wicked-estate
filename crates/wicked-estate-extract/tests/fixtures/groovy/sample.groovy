package com.example

import groovy.transform.CompileStatic

@CompileStatic
class ReportGenerator {
    private final DataSource dataSource

    ReportGenerator(DataSource dataSource) {
        this.dataSource = dataSource
    }

    Map<String, Integer> summarize(String category) {
        def rows = dataSource.query("SELECT * FROM events WHERE category = ?", [category])
        rows.groupBy { it.type }.collectEntries { type, items ->
            [(type): items.size()]
        }
    }

    String renderHtml(Map<String, Integer> summary) {
        def sb = new StringBuilder('<table>')
        summary.each { k, v ->
            sb.append("<tr><td>${k}</td><td>${v}</td></tr>")
        }
        sb.append('</table>')
        sb.toString()
    }

    void writeReport(String path, String category) {
        def summary = summarize(category)
        new File(path).text = renderHtml(summary)
    }
}

def add(a, b) { a + b }
