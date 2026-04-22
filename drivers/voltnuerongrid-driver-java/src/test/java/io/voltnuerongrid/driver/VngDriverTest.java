package io.voltnuerongrid.driver;

import org.junit.jupiter.api.Test;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

class VngDriverTest {
    @Test
    void buildsHealthRequest() {
        VngDriver driver = new VngDriver("http://127.0.0.1:8080");
        VngDriver.Request req = driver.buildHealthRequest();
        assertEquals("GET", req.method());
        assertEquals("http://127.0.0.1:8080/health", req.url());
    }

    @Test
    void rejectsEmptySqlAnalyzePayload() {
        VngDriver driver = new VngDriver("http://127.0.0.1:8080");
        assertThrows(IllegalArgumentException.class, () -> driver.buildSqlAnalyzeRequest(""));
    }
}
