// Uses the MSVC RTTI that ApplyRtti.java labelled to attach otherwise unnamed functions to
// the C++ classes they belong to. Run after ApplyRtti.java, before ExportFNV.java.
//
// Virtual functions: every function referenced from a class vftable is moved into the
// namespace of the most basic class whose vftable contains it and named vfunc_<slot>
// (vfunc_<slot>_at_<offset> for secondary vftables under multiple inheritance), e.g.
// TESForm::vfunc_12. A function shared by unrelated classes (identical code folded by the
// linker) is left alone, since no single class owns it.
//
// Constructors/destructors: a function that stores a class's primary vftable address into
// memory is a constructor or destructor of that class (or of a class derived from it, when
// a base constructor was inlined). It is moved into the most derived such class and named
// ctor_or_dtor_<address>. This is a heuristic, hence the deliberately vague name.
//
// Only functions that still have Ghidra's default FUN_ names are touched.
// @category FNV
import java.util.*;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.symbol.*;

public class NameClassFunctions extends GhidraScript {

	/** One vftable: its class, its RTTI depth (number of bases incl. itself), its subobject offset. */
	private record VTable(Address addr, Namespace cls, long typeDesc, int depth, int offset,
			Set<Long> baseTypeDescs) {}

	private record Slot(VTable vt, int index) {}

	private Memory mem;

	private long ptr(long a) throws Exception {
		return mem.getInt(toAddr(a)) & 0xFFFFFFFFL;
	}

	@Override
	protected void run() throws Exception {
		mem = currentProgram.getMemory();
		SymbolTable st = currentProgram.getSymbolTable();
		Listing listing = currentProgram.getListing();
		FunctionManager fm = currentProgram.getFunctionManager();

		List<VTable> vtables = new ArrayList<>();
		Map<Long, VTable> primaryByTypeDesc = new HashMap<>();
		for (Symbol s : st.getSymbolIterator("vftable", true)) {
			try {
				long col = ptr(s.getAddress().getOffset() - 4);
				int offset = mem.getInt(toAddr(col + 4));
				long td = ptr(col + 12);
				long chd = ptr(col + 16);
				int numBases = mem.getInt(toAddr(chd + 8));
				long bca = ptr(chd + 12);
				Set<Long> bases = new HashSet<>();
				for (int i = 0; i < numBases; i++) {
					bases.add(ptr(ptr(bca + 4L * i)));
				}
				VTable vt = new VTable(s.getAddress(), s.getParentNamespace(), td, numBases, offset,
					bases);
				vtables.add(vt);
				if (offset == 0) primaryByTypeDesc.put(td, vt);
			}
			catch (Exception e) {
				// Not backed by a readable complete object locator (e.g. type_info); skip.
			}
		}
		println("vftables with RTTI: " + vtables.size());

		// ---- Virtual functions ----
		Map<Function, List<Slot>> slots = new LinkedHashMap<>();
		for (VTable vt : vtables) {
			Data d = listing.getDataAt(vt.addr());
			if (d == null || !d.isArray()) continue;
			for (int i = 0; i < d.getNumComponents(); i++) {
				Object v = d.getComponent(i).getValue();
				if (!(v instanceof Address a)) continue;
				Function f = fm.getFunctionAt(a);
				if (f == null) continue;
				slots.computeIfAbsent(f, k -> new ArrayList<>()).add(new Slot(vt, i));
			}
		}

		int vfuncs = 0, shared = 0;
		for (Map.Entry<Function, List<Slot>> e : slots.entrySet()) {
			monitor.checkCancelled();
			Function f = e.getKey();
			if (f.getSymbol().getSource() != SourceType.DEFAULT) continue;
			List<Slot> uses = e.getValue();
			// Owner: the shallowest class, preferring primary vftables; it must be a base of
			// (or the same as) every other class using the function.
			Slot owner = Collections.min(uses, Comparator
					.comparingInt((Slot x) -> x.vt().depth())
					.thenComparingInt(x -> x.vt().offset() == 0 ? 0 : 1)
					.thenComparing(x -> x.vt().cls().getName(true)));
			boolean related = uses.stream()
					.allMatch(x -> x.vt().baseTypeDescs().contains(owner.vt().typeDesc()));
			if (!related) {
				shared++;
				continue;
			}
			String name = "vfunc_" + owner.index() +
				(owner.vt().offset() == 0 ? "" : "_at_" + Integer.toHexString(owner.vt().offset()));
			rename(f, owner.vt().cls(), name);
			vfuncs++;
		}
		println("virtual functions named: " + vfuncs + " (left alone, shared by unrelated classes: " +
			shared + ")");

		// ---- Constructors / destructors ----
		Map<Function, Set<VTable>> writers = new HashMap<>();
		ReferenceManager rm = currentProgram.getReferenceManager();
		for (VTable vt : primaryByTypeDesc.values()) {
			for (Reference r : rm.getReferencesTo(vt.addr())) {
				Instruction ins = listing.getInstructionAt(r.getFromAddress());
				if (ins == null || !ins.getMnemonicString().equals("MOV") ||
					ins.getOperandRefType(0) == null || !ins.getOperandRefType(0).isWrite()) {
					continue;
				}
				Function f = fm.getFunctionContaining(r.getFromAddress());
				if (f != null) writers.computeIfAbsent(f, k -> new HashSet<>()).add(vt);
			}
		}
		int ctors = 0;
		for (Map.Entry<Function, Set<VTable>> e : writers.entrySet()) {
			Function f = e.getKey();
			if (f.getSymbol().getSource() != SourceType.DEFAULT) continue;
			VTable most = Collections.max(e.getValue(), Comparator.comparingInt(VTable::depth));
			// Every other vftable written must belong to a base of the chosen class.
			if (!e.getValue().stream().allMatch(v -> most.baseTypeDescs().contains(v.typeDesc()))) {
				continue;
			}
			rename(f, most.cls(), "ctor_or_dtor_" + f.getEntryPoint());
			ctors++;
		}
		println("constructors/destructors named: " + ctors);
	}

	private void rename(Function f, Namespace ns, String name) throws Exception {
		SymbolTable st = currentProgram.getSymbolTable();
		String unique = name;
		if (!st.getSymbols(unique, ns).isEmpty()) unique = name + "_" + f.getEntryPoint();
		f.getSymbol().setNameAndNamespace(unique, ns, SourceType.ANALYSIS);
	}
}
