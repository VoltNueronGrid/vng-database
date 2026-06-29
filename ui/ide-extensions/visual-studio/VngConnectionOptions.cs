using Microsoft.VisualStudio.Shell;

namespace VoltNueronGrid.VS.OptionPages
{
    /// <summary>
    /// Tools > Options > VoltNueronGrid > Connection settings page.
    /// </summary>
    public class VngConnectionOptions : DialogPage
    {
        [System.ComponentModel.Category("VoltNueronGrid Connection")]
        [System.ComponentModel.DisplayName("Host")]
        [System.ComponentModel.Description("VoltNueronGrid server hostname or IP address.")]
        public string Host { get; set; } = "127.0.0.1";

        [System.ComponentModel.Category("VoltNueronGrid Connection")]
        [System.ComponentModel.DisplayName("Port")]
        [System.ComponentModel.Description("VoltNueronGrid HTTP port (default: 8080).")]
        public int Port { get; set; } = 8080;

        [System.ComponentModel.Category("VoltNueronGrid Connection")]
        [System.ComponentModel.DisplayName("Admin API Key")]
        [System.ComponentModel.Description("x-vng-admin-key header value.")]
        public string AdminApiKey { get; set; } = "";

        [System.ComponentModel.Category("VoltNueronGrid Connection")]
        [System.ComponentModel.DisplayName("Default Database")]
        [System.ComponentModel.Description("Default database name for SQL queries.")]
        public string DefaultDatabase { get; set; } = "default";

        [System.ComponentModel.Category("VoltNueronGrid Connection")]
        [System.ComponentModel.DisplayName("TLS Enabled")]
        [System.ComponentModel.Description("Use HTTPS instead of HTTP.")]
        public bool TlsEnabled { get; set; } = false;
    }
}
