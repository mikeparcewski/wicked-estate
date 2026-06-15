#include <Arduino.h>

// Pin assignments
const int LED_PIN    = 13;
const int SENSOR_PIN = A0;
const int BUZZER_PIN = 8;

// Thresholds
const int SENSOR_HIGH_THRESHOLD = 800;
const int SENSOR_LOW_THRESHOLD  = 200;

// Reads the analogue sensor and returns a smoothed value (moving average).
int readSmoothed(int pin, int samples) {
    long total = 0;
    for (int i = 0; i < samples; i++) {
        total += analogRead(pin);
        delay(2);
    }
    return (int)(total / samples);
}

// Blinks the LED a given number of times at the specified period (ms).
void blinkLed(int pin, int times, int period) {
    for (int i = 0; i < times; i++) {
        digitalWrite(pin, HIGH);
        delay(period / 2);
        digitalWrite(pin, LOW);
        delay(period / 2);
    }
}

// Sounds the buzzer for the given duration (ms) at the given frequency (Hz).
void buzz(int pin, int freq, int duration) {
    tone(pin, freq, duration);
    delay(duration + 10);
    noTone(pin);
}

void setup() {
    Serial.begin(9600);
    pinMode(LED_PIN,    OUTPUT);
    pinMode(BUZZER_PIN, OUTPUT);
    digitalWrite(LED_PIN, LOW);

    Serial.println("Sensor monitor ready.");
    blinkLed(LED_PIN, 3, 200);
}

void loop() {
    int value = readSmoothed(SENSOR_PIN, 8);
    Serial.print("Sensor: ");
    Serial.println(value);

    if (value > SENSOR_HIGH_THRESHOLD) {
        digitalWrite(LED_PIN, HIGH);
        buzz(BUZZER_PIN, 1000, 150);
        Serial.println("WARNING: high reading");
    } else if (value < SENSOR_LOW_THRESHOLD) {
        digitalWrite(LED_PIN, LOW);
        buzz(BUZZER_PIN, 440, 80);
        Serial.println("NOTICE: low reading");
    } else {
        // Normal range — heartbeat blink
        blinkLed(LED_PIN, 1, 100);
    }

    delay(500);
}
