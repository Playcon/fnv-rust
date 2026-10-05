// Exports decompiled C and disassembly for a set of root functions and their callees up
// to a given depth, plus the first 16 bytes at every data address they reference.
//   dec_<addr>.c, dis_<addr>.txt per function; data_refs.txt; index.tsv (address, depth, name)
// Usage (headless): -postScript ExportCallTree.java <out_dir> <depth> <addr> [<addr> ...]
// In the GUI it asks for the folder, depth and a comma-separated list of root addresses.
// @category FNV
import java.io.*;
import java.nio.charset.StandardCharsets;
import java.util.*;

import ghidra.app.decompiler.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.Reference;

public class ExportCallTree extends GhidraScript {

	private static PrintWriter open(File f) throws IOException {
		return new PrintWriter(new BufferedWriter(
			new OutputStreamWriter(new FileOutputStream(f), StandardCharsets.UTF_8)));
	}

	@Override
	protected void run() throws Exception {
		String[] args = getScriptArgs();
		File out;
		int maxDepth;
		List<String> roots = new ArrayList<>();
		if (args.length >= 3) {
			out = new File(args[0]);
			maxDepth = Integer.parseInt(args[1]);
			roots.addAll(Arrays.asList(args).subList(2, args.length));
		}
		else {
			out = askDirectory("Export folder", "Choose");
			maxDepth = askInt("Depth", "Callee levels to follow");
			for (String s : askString("Roots", "Comma-separated root addresses").split(",")) {
				roots.add(s.trim());
			}
		}
		out.mkdirs();

		DecompInterface di = new DecompInterface();
		di.openProgram(currentProgram);
		FunctionManager fm = currentProgram.getFunctionManager();
		Listing listing = currentProgram.getListing();

		Map<Address, Integer> depth = new LinkedHashMap<>();
		Deque<Address> todo = new ArrayDeque<>();
		for (String r : roots) {
			Address a = toAddr(r);
			depth.put(a, 0);
			todo.add(a);
		}
		Set<Address> done = new LinkedHashSet<>();
		TreeSet<Address> data = new TreeSet<>();

		try (PrintWriter index = open(new File(out, "index.tsv"))) {
			index.println("address\tdepth\tname");
			while (!todo.isEmpty()) {
				Address a = todo.poll();
				if (!done.add(a)) continue;
				String tag = String.format("%08x", a.getOffset());
				Function f = fm.getFunctionAt(a);
				if (f == null || f.isExternal()) {
					println("skipped " + tag);
					continue;
				}
				index.println(tag + "\t" + depth.get(a) + "\t" + f.getName(true));
				DecompileResults res = di.decompileFunction(f, 180, monitor);
				try (PrintWriter w = open(new File(out, "dec_" + tag + ".c"))) {
					if (res.decompileCompleted()) {
						w.print(res.getDecompiledFunction().getC());
					}
					else {
						w.println("// decompile failed: " + res.getErrorMessage());
					}
				}
				try (PrintWriter w = open(new File(out, "dis_" + tag + ".txt"))) {
					for (Instruction ins : listing.getInstructions(f.getBody(), true)) {
						w.println(ins.getAddress() + "  " + ins);
						for (Reference ref : ins.getReferencesFrom()) {
							if (ref.getReferenceType().isData()) data.add(ref.getToAddress());
						}
					}
				}
				if (depth.get(a) < maxDepth) {
					for (Function c : f.getCalledFunctions(monitor)) {
						Address ca = c.getEntryPoint();
						if (!depth.containsKey(ca)) {
							depth.put(ca, depth.get(a) + 1);
							todo.add(ca);
						}
					}
				}
			}
		}
		finally {
			di.dispose();
		}

		try (PrintWriter w = open(new File(out, "data_refs.txt"))) {
			for (Address d : data) {
				try {
					byte[] b = getBytes(d, 16);
					StringBuilder sb = new StringBuilder();
					for (byte x : b) sb.append(String.format(" %02x", x & 0xff));
					w.println(d + " " + sb);
				}
				catch (Exception e) {
					w.println(d + "  <unreadable>");
				}
			}
		}
		println("exported " + done.size() + " functions to " + out);
	}
}
