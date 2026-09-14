Set shell = CreateObject("WScript.Shell")
Set files = CreateObject("Scripting.FileSystemObject")
directory = files.GetParentFolderName(WScript.ScriptFullName)
shell.Run Chr(34) & directory & "\latch.exe" & Chr(34) & " start --startup", 0, False
