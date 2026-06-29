using System.Net.Http;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;

namespace VoltNueronGrid.VS.VoltNueronGridClient
{
    /// <summary>
    /// Typed HTTP client for the VoltNueronGrid REST API.
    /// Host/port/key are read from VngConnectionOptions at call time.
    /// </summary>
    public class VngApiClient
    {
        private static readonly HttpClient _http = new HttpClient();

        private readonly string _baseUrl;
        private readonly string _adminKey;

        public VngApiClient(string host = "127.0.0.1", int port = 8080, string adminKey = "", bool tls = false)
        {
            _baseUrl = $"{(tls ? "https" : "http")}://{host}:{port}";
            _adminKey = adminKey;
        }

        /// <summary>Execute a SQL batch; returns the raw JSON response.</summary>
        public async Task<JsonDocument> ExecuteSqlAsync(string sql, string database = "")
        {
            var payload = JsonSerializer.Serialize(new
            {
                sql_batch = sql,
                database = string.IsNullOrEmpty(database) ? "default" : database
            });
            using var req = new HttpRequestMessage(HttpMethod.Post, $"{_baseUrl}/api/v1/sql/execute")
            {
                Content = new StringContent(payload, Encoding.UTF8, "application/json")
            };
            req.Headers.Add("x-vng-admin-key", _adminKey);
            using var resp = await _http.SendAsync(req);
            var body = await resp.Content.ReadAsStringAsync();
            return JsonDocument.Parse(body);
        }

        /// <summary>Fetch catalog entries from /api/v1/catalog/list.</summary>
        public async Task<JsonDocument> ListSchemaAsync()
        {
            using var req = new HttpRequestMessage(HttpMethod.Get, $"{_baseUrl}/api/v1/catalog/list");
            req.Headers.Add("x-vng-admin-key", _adminKey);
            using var resp = await _http.SendAsync(req);
            var body = await resp.Content.ReadAsStringAsync();
            return JsonDocument.Parse(body);
        }

        /// <summary>Health-check: returns true when the server is reachable.</summary>
        public async Task<bool> HealthCheckAsync()
        {
            try
            {
                using var req = new HttpRequestMessage(HttpMethod.Get, $"{_baseUrl}/api/v1/health");
                req.Headers.Add("x-vng-admin-key", _adminKey);
                using var resp = await _http.SendAsync(req);
                return resp.IsSuccessStatusCode;
            }
            catch { return false; }
        }
    }
}
