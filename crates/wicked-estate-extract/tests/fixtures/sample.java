package com.example;

import java.util.ArrayList;
import java.util.List;

@interface Marker {
    String value();
    int priority() default 0;
}

public class DataProcessor {
    public static final int MAX_ITEMS = 1000;
    public static final String VERSION = "1.0";

    private final List<String> items = new ArrayList<>();
    private int processCount;

    public void add(String item) {
        items.add(item);
    }

    public String process() {
        return format(items);
    }

    private String format(List<String> data) {
        return String.join(", ", data);
    }

    public int count() {
        return items.size();
    }
}

class PipelineRunner {
    public static void run(DataProcessor processor) {
        processor.add("alpha");
        processor.add("beta");
        String result = processor.process();
        System.out.println(result);
    }
}
