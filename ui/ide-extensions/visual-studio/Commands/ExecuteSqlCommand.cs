using System;
using System.ComponentModel.Design;
using Microsoft.VisualStudio.Shell;
using Task = System.Threading.Tasks.Task;

namespace VoltNueronGrid.VS.Commands
{
    /// <summary>Execute SQL command — sends selected text to VoltNueronGrid.</summary>
    internal sealed class ExecuteSqlCommand
    {
        public const int CommandId = 0x0100;
        public static readonly Guid CommandSet = new Guid("c1d2e3f4-5678-90ab-cdef-012345678901");

        private readonly AsyncPackage _package;

        private ExecuteSqlCommand(AsyncPackage package, IMenuCommandService commandService)
        {
            _package = package;
            var id = new CommandID(CommandSet, CommandId);
            commandService.AddCommand(new MenuCommand(Execute, id));
        }

        public static async Task InitializeAsync(AsyncPackage package)
        {
            await ThreadHelper.JoinableTaskFactory.SwitchToMainThreadAsync(package.DisposalToken);
            var svc = await package.GetServiceAsync(typeof(IMenuCommandService)) as IMenuCommandService;
            if (svc != null) new ExecuteSqlCommand(package, svc);
        }

        private void Execute(object sender, EventArgs e)
        {
            ThreadHelper.ThrowIfNotOnUIThread();
            // Open the VoltNueronGrid tool window to the SQL Editor tab
            _package.JoinableTaskFactory.RunAsync(async () =>
            {
                await _package.ShowToolWindowAsync(typeof(ToolWindows.VngToolWindow), 0, true, _package.DisposalToken);
            });
        }
    }
}
