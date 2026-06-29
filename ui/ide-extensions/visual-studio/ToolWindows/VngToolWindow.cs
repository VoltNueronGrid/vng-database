using System;
using System.Runtime.InteropServices;
using Microsoft.VisualStudio.Shell;

namespace VoltNueronGrid.VS.ToolWindows
{
    /// <summary>
    /// VoltNueronGrid tool window — hosts the SQL editor, schema browser, and result grid.
    /// </summary>
    [Guid("b2c3d4e5-f678-4901-abcd-ef1234567890")]
    public class VngToolWindow : ToolWindowPane
    {
        public VngToolWindow() : base(null)
        {
            Caption = "VoltNueronGrid";
            Content = new VngToolWindowControl();
        }
    }
}
