using System;
using System.ComponentModel.Design;
using Microsoft.VisualStudio.Shell;
using Task = System.Threading.Tasks.Task;

namespace VoltNueronGrid.VS.Commands
{
    /// <summary>Open the VoltNueronGrid tool window command.</summary>
    internal sealed class OpenConnectionCommand
    {
        public const int CommandId = 0x0101;
        public static readonly Guid CommandSet = new Guid("c1d2e3f4-5678-90ab-cdef-012345678901");

        private readonly AsyncPackage _package;

        private OpenConnectionCommand(AsyncPackage package, IMenuCommandService commandService)
        {
            _package = package;
            var id = new CommandID(CommandSet, CommandId);
            commandService.AddCommand(new MenuCommand(Execute, id));
        }

        public static async Task InitializeAsync(AsyncPackage package)
        {
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync(package.DisposalToken);
            var svc = await package.GetServiceAsync(typeof(IMenuCommandService)) as IMenuCommandService;
            if (svc != null) new OpenConnectionCommand(package, svc);
        }

        private void Execute(object sender, EventArgs e)
        {
            ThreadHelper.ThrowIfNotOnUIThread();
            _package.JoinableTaskFactory.RunAsync(async () =>
            {
                await _package.ShowToolWindowAsync(typeof(ToolWindows.VngToolWindow), 0, true, _package.DisposalToken);
            });
        }
    }
}
