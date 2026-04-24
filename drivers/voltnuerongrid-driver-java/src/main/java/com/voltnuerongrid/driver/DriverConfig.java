package com.voltnuerongrid.driver;

/**
 * Configuration for the VoltNueronGrid Java driver.
 * Use the {@link Builder} to construct instances.
 */
public final class DriverConfig {

    /** Base URL for REST endpoints, e.g. {@code http://localhost:8080}. */
    public final String baseUrl;

    /** Session identifier for multiplexing. */
    public final String sessionId;

    /** Operating mode: {@code "admin"}, {@code "operator"}, or {@code "tenant"}. */
    public final String mode;

    /** Required for admin and operator modes. */
    public String adminApiKey;

    /** Required for operator mode. */
    public String operatorId;

    /** Required for tenant mode. */
    public String tenantId;

    /** Optional user identifier (tenant mode). */
    public String userId;

    /** Optional routing hint header. */
    public String routeHint;

    /** Per-request timeout in milliseconds. Default: 30000. */
    public int requestTimeoutMs = 30000;

    /** Maximum number of retry attempts after transient failures. Default: 2. */
    public int maxRetries = 2;

    private DriverConfig(Builder builder) {
        this.baseUrl = builder.baseUrl;
        this.sessionId = builder.sessionId;
        this.mode = builder.mode;
        this.adminApiKey = builder.adminApiKey;
        this.operatorId = builder.operatorId;
        this.tenantId = builder.tenantId;
        this.userId = builder.userId;
        this.routeHint = builder.routeHint;
        this.requestTimeoutMs = builder.requestTimeoutMs;
        this.maxRetries = builder.maxRetries;
    }

    /**
     * Validates this configuration and returns an error message, or {@code null} if valid.
     */
    public String validate() {
        if (baseUrl == null || baseUrl.trim().isEmpty()) {
            return "baseUrl must not be empty";
        }
        if (sessionId == null || sessionId.trim().isEmpty()) {
            return "sessionId must not be empty";
        }
        if (mode == null || mode.trim().isEmpty()) {
            return "mode must not be empty";
        }
        switch (mode) {
            case "admin":
                if (adminApiKey == null || adminApiKey.trim().isEmpty()) {
                    return "admin mode requires adminApiKey";
                }
                break;
            case "operator":
                if (adminApiKey == null || adminApiKey.trim().isEmpty()) {
                    return "operator mode requires adminApiKey";
                }
                if (operatorId == null || operatorId.trim().isEmpty()) {
                    return "operator mode requires operatorId";
                }
                break;
            case "tenant":
                if (tenantId == null || tenantId.trim().isEmpty()) {
                    return "tenant mode requires tenantId";
                }
                break;
            default:
                return "mode must be one of: admin, operator, tenant";
        }
        if (requestTimeoutMs < 100) {
            return "requestTimeoutMs must be >= 100";
        }
        if (maxRetries < 0 || maxRetries > 20) {
            return "maxRetries must be between 0 and 20";
        }
        return null;
    }

    /** Throws {@link DriverError} if this configuration is invalid. */
    public void validateOrThrow() {
        String error = validate();
        if (error != null) {
            throw new DriverError(DriverError.Kind.VALIDATION, error, 0);
        }
    }

    @Override
    public String toString() {
        return "DriverConfig{baseUrl='" + baseUrl + "', sessionId='" + sessionId + "', mode='" + mode + "'}";
    }

    /** Fluent builder for {@link DriverConfig}. */
    public static final class Builder {
        private String baseUrl;
        private String sessionId;
        private String mode;
        private String adminApiKey;
        private String operatorId;
        private String tenantId;
        private String userId;
        private String routeHint;
        private int requestTimeoutMs = 30000;
        private int maxRetries = 2;

        public Builder baseUrl(String baseUrl) {
            this.baseUrl = baseUrl;
            return this;
        }

        public Builder sessionId(String sessionId) {
            this.sessionId = sessionId;
            return this;
        }

        public Builder mode(String mode) {
            this.mode = mode;
            return this;
        }

        public Builder adminApiKey(String adminApiKey) {
            this.adminApiKey = adminApiKey;
            return this;
        }

        public Builder operatorId(String operatorId) {
            this.operatorId = operatorId;
            return this;
        }

        public Builder tenantId(String tenantId) {
            this.tenantId = tenantId;
            return this;
        }

        public Builder userId(String userId) {
            this.userId = userId;
            return this;
        }

        public Builder routeHint(String routeHint) {
            this.routeHint = routeHint;
            return this;
        }

        public Builder requestTimeoutMs(int requestTimeoutMs) {
            this.requestTimeoutMs = requestTimeoutMs;
            return this;
        }

        public Builder maxRetries(int maxRetries) {
            this.maxRetries = maxRetries;
            return this;
        }

        public DriverConfig build() {
            return new DriverConfig(this);
        }
    }
}
