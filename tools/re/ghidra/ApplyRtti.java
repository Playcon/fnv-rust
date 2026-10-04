// Runs Ghidra's MSVC RTTI analyzer on the current program so classes with run-time type
// information get namespaces and labelled vftables. FalloutNV.exe only carries RTTI for a
// few dozen classes, and the analyzer is not part of the default headless analysis.
// The SteamStub-decrypted executable is not recognised as a Visual Studio binary at import,
// which makes Ghidra refuse to create RTTI structures, so the compiler is set explicitly.
// The analyzer also records "RTTI Found" and skips later runs, so that flag is cleared first.
// @category FNV
import ghidra.app.plugin.prototype.MicrosoftCodeAnalyzerPlugin.RttiAnalyzer;
import ghidra.app.script.GhidraScript;
import ghidra.app.util.importer.MessageLog;
import ghidra.app.util.opinion.PeLoader.CompilerOpinion.CompilerEnum;
import ghidra.program.model.listing.Program;

public class ApplyRtti extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String vs = CompilerEnum.VisualStudio.toString();
		println("Compiler was: " + currentProgram.getCompiler());
		if (!vs.equals(currentProgram.getCompiler())) currentProgram.setCompiler(vs);
		currentProgram.getOptions(Program.PROGRAM_INFO).removeOption(RttiAnalyzer.RTTI_FOUND_OPTION);
		MessageLog log = new MessageLog();
		boolean ok = new RttiAnalyzer().added(currentProgram, currentProgram.getMemory(), monitor, log);
		int tds = 0;
		for (var it = currentProgram.getSymbolTable().getSymbolIterator("RTTI_Type_Descriptor", true); it
				.hasNext(); it.next()) {
			tds++;
		}
		println("RTTI analyzer finished: " + ok + ", type descriptors labelled: " + tds);
		if (log.hasMessages()) println(log.toString());
	}
}
