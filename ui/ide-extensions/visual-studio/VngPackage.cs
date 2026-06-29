using System;
using System.Runtime.InteropServices;
using System.Threading;
using Microsoft.VisualStudio.Shell;
using Task = System.Threading.Tasks.Task;

namespace VoltNueronGrid.VS
{
    /// <summary>
    /// VS Package entry point for VoltNueronGrid Tools extension.
    /// Registers the tool window, menu commands, and settings page.
    /// </summary>
    [PackageRegistration(UseManagedResourcesOnly = true, AllowsBackgroundLoading = true)]
    [Guid(VngPackage.PackageGuidString)]
    [ProvideToolWindow(typeof(ToolWindows.VngToolWindow), Style = VsDockStyle.Tabbed, Window = "DocumentWell")]
    [ProvideMenuResource("Menus.ctmenu", 1)]
    [ProvideOptionPage(typeof(OptionPages.VngConnectionOptions), "VoltNueronGrid", "Connection", 0, 0, true)]
    public sealed class VngPackage : AsyncPackage
    {
        public const string PackageGuidString = "a3b7c2e1-4f56-4d89-9abc-def012345678";

        protected override async Task InitializeAsync(CancellationToken cancellationToken, IProgress<ServiceProgressData> progress)
        {
            await base.InitializeAsync(cancellationToken, progress);
            await JoinableTaskFactory.SwitchToMainThreadAsync(cancellationToken);
            await Commands.ExecuteSqlCommand.InitializeAsync(this);
            await Commands.OpenConnectionCommand.InitializeAsync(this);
        }
    }
}
