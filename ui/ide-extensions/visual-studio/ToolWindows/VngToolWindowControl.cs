using System;
using System.Collections.Generic;
using System.Data;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Controls;
using VoltNueronGrid.VS.VoltNueronGridClient;

namespace VoltNueronGrid.VS.ToolWindows
{
    /// <summary>
    /// WPF control embedded in the VoltNueronGrid tool window.
    /// Contains tabs: Connection, Schema Browser, SQL Editor, Results.
    /// </summary>
    public class VngToolWindowControl : System.Windows.Controls.UserControl
    {
        // Connection fields
        private TextBox _hostBox = new TextBox { Text = "127.0.0.1", Width = 160 };
        private TextBox _portBox = new TextBox { Text = "8080", Width = 60 };
        private PasswordBox _keyBox = new PasswordBox { Width = 200 };
        private TextBox _dbBox = new TextBox { Text = "default", Width = 120 };

        // Schema browser
        private TreeView _schemaTree = new TreeView();

        // SQL editor + results
        private TextBox _sqlEditor = new TextBox { AcceptsReturn = true, AcceptsTab = true, FontFamily = new System.Windows.Media.FontFamily("Consolas"), Height = 120 };
        private DataGrid _resultGrid = new DataGrid { AutoGenerateColumns = true, IsReadOnly = true };
        private TextBlock _statusLabel = new TextBlock { Text = "Ready." };

        public VngToolWindowControl()
        {
            var tabs = new TabControl();

            // --- Connection tab ---
            var connPanel = new StackPanel { Margin = new Thickness(8) };
            connPanel.Children.Add(MakeRow("Host:", _hostBox));
            connPanel.Children.Add(MakeRow("Port:", _portBox));
            connPanel.Children.Add(MakeRow("Admin Key:", _keyBox));
            connPanel.Children.Add(MakeRow("Database:", _dbBox));
            var testBtn = new Button { Content = "Test Connection", Width = 140, Margin = new Thickness(0, 6, 0, 0) };
            testBtn.Click += async (s, e) => await TestConnectionAsync();
            connPanel.Children.Add(testBtn);
            tabs.Items.Add(new TabItem { Header = "Connection", Content = connPanel });

            // --- Schema tab ---
            var schemaPanel = new DockPanel();
            var refreshBtn = new Button { Content = "Refresh Schema", Width = 120 };
            DockPanel.SetDock(refreshBtn, Dock.Top);
            refreshBtn.Click += async (s, e) => await RefreshSchemaAsync();
            schemaPanel.Children.Add(refreshBtn);
            schemaPanel.Children.Add(_schemaTree);
            tabs.Items.Add(new TabItem { Header = "Schema", Content = schemaPanel });

            // --- SQL Editor tab ---
            var sqlPanel = new DockPanel();
            var runBtn = new Button { Content = "▶ Run", Width = 80 };
            DockPanel.SetDock(runBtn, Dock.Top);
            runBtn.Click += async (s, e) => await ExecuteSqlAsync();
            sqlPanel.Children.Add(runBtn);
            sqlPanel.Children.Add(_sqlEditor);
            tabs.Items.Add(new TabItem { Header = "SQL Editor", Content = sqlPanel });

            // --- Results tab ---
            var resultsPanel = new DockPanel();
            DockPanel.SetDock(_statusLabel, Dock.Top);
            resultsPanel.Children.Add(_statusLabel);
            resultsPanel.Children.Add(_resultGrid);
            tabs.Items.Add(new TabItem { Header = "Results", Content = resultsPanel });

            Content = tabs;
        }

        private VngApiClient MakeClient() =>
            new VngApiClient(_hostBox.Text, int.TryParse(_portBox.Text, out int p) ? p : 8080, _keyBox.Password);

        private async Task TestConnectionAsync()
        {
            var ok = await MakeClient().HealthCheckAsync();
            _statusLabel.Text = ok ? "Connection OK ✓" : "Connection FAILED ✗";
        }

        private async Task RefreshSchemaAsync()
        {
            try
            {
                var doc = await MakeClient().ListSchemaAsync();
                _schemaTree.Items.Clear();
                if (doc.RootElement.ValueKind == System.Text.Json.JsonValueKind.Array)
                {
                    var groups = new Dictionary<string, TreeViewItem>();
                    foreach (var entry in doc.RootElement.EnumerateArray())
                    {
                        var kind = entry.TryGetProperty("object_kind", out var k) ? k.GetString() ?? "other" : "other";
                        var name = entry.TryGetProperty("object_name", out var n) ? n.GetString() ?? "?" : "?";
                        if (!groups.TryGetValue(kind, out var groupNode))
                        {
                            groupNode = new TreeViewItem { Header = kind, IsExpanded = true };
                            groups[kind] = groupNode;
                            _schemaTree.Items.Add(groupNode);
                        }
                        groupNode.Items.Add(new TreeViewItem { Header = name });
                    }
                }
                _statusLabel.Text = "Schema loaded.";
            }
            catch (Exception ex) { _statusLabel.Text = $"Error: {ex.Message}"; }
        }

        private async Task ExecuteSqlAsync()
        {
            try
            {
                var doc = await MakeClient().ExecuteSqlAsync(_sqlEditor.Text.Trim(), _dbBox.Text.Trim());
                // Try to show oltp_rows
                var table = new DataTable();
                if (doc.RootElement.TryGetProperty("oltp_rows", out var rows) && rows.ValueKind == System.Text.Json.JsonValueKind.Array)
                {
                    bool colsAdded = false;
                    foreach (var row in rows.EnumerateArray())
                    {
                        if (row.TryGetProperty("data", out var data))
                        {
                            if (!colsAdded)
                            {
                                foreach (var col in data.EnumerateObject())
                                    table.Columns.Add(col.Name);
                                colsAdded = true;
                            }
                            var dr = table.NewRow();
                            foreach (var col in data.EnumerateObject())
                                dr[col.Name] = col.Value.GetRawText().Trim('"');
                            table.Rows.Add(dr);
                        }
                    }
                }
                else
                {
                    table.Columns.Add("status");
                    var dr = table.NewRow();
                    dr["status"] = doc.RootElement.TryGetProperty("status", out var s) ? s.GetString() : "ok";
                    table.Rows.Add(dr);
                }
                _resultGrid.ItemsSource = table.DefaultView;
                _statusLabel.Text = $"{table.Rows.Count} row(s) returned.";
            }
            catch (Exception ex) { _statusLabel.Text = $"Error: {ex.Message}"; }
        }

        private static StackPanel MakeRow(string label, UIElement ctrl)
        {
            var row = new StackPanel { Orientation = Orientation.Horizontal, Margin = new Thickness(0, 4, 0, 0) };
            row.Children.Add(new TextBlock { Text = label, Width = 90, VerticalAlignment = VerticalAlignment.Center });
            row.Children.Add(ctrl);
            return row;
        }
    }
}
