package com.example.build

import groovy.transform.CompileStatic

@CompileStatic
class TaskRunner {

    static final int MAX_RETRIES = 3

    private final List<String> log = []
    private int runCount = 0

    String run(String taskName, Closure<String> task) {
        runCount++
        def result = execute(taskName, task)
        record(taskName, result)
        return result
    }

    private String execute(String name, Closure<String> task) {
        try {
            return task.call()
        } catch (Exception e) {
            return "ERROR: ${e.message}"
        }
    }

    private void record(String name, String result) {
        log << "[${name}] ${result}"
    }

    int getRunCount() { runCount }

    List<String> getLog() { Collections.unmodifiableList(log) }
}

def runner = new TaskRunner()
def result = runner.run("compile") { "compiled" }
println runner.getLog()
