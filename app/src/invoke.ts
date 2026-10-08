// Offline transaction building. `contract.Client` needs an RPC to simulate
// before it can produce a transaction; this module builds the unsigned
// InvokeHostFunction transaction directly from the spec, which is what the
// offline tests exercise and what the static web page mirrors for `status`.

import {
  Account,
  Address,
  BASE_FEE,
  Contract,
  TransactionBuilder,
  type Transaction,
  type contract,
  type xdr,
} from "@stellar/stellar-sdk";

export interface BuildParams {
  contractId: string;
  method: string;
  args: Record<string, unknown>;
  spec: contract.Spec;
  sourcePublicKey: string;
  /** Account sequence number as a decimal string. */
  sequence: string;
  networkPassphrase: string;
  fee?: string;
  timeoutSec?: number;
}

/** Type-directed argument encoding straight from the contract spec. */
export function encodeArgs(spec: contract.Spec, method: string, args: Record<string, unknown>): xdr.ScVal[] {
  return spec.funcArgsToScVals(method, args);
}

/** Builds an unsigned, unsimulated transaction invoking `method` on the contract. */
export function buildUnsignedTx(p: BuildParams): Transaction {
  const scArgs = encodeArgs(p.spec, p.method, p.args);
  const source = new Account(p.sourcePublicKey, p.sequence);
  const op = new Contract(p.contractId).call(p.method, ...scArgs);
  return new TransactionBuilder(source, { fee: p.fee ?? BASE_FEE, networkPassphrase: p.networkPassphrase })
    .addOperation(op)
    .setTimeout(p.timeoutSec ?? 300)
    .build();
}

export interface InvocationSummary {
  contractId: string;
  method: string;
  argCount: number;
}

/** Decodes the first operation of a transaction back into contract id, function name and arg count. */
export function describeInvocation(tx: Transaction): InvocationSummary {
  const op = tx.operations[0];
  if (!op || op.type !== "invokeHostFunction") {
    throw new Error("not an invokeHostFunction transaction");
  }
  const func = op.func as unknown as Record<string, unknown>;
  let ic: Record<string, unknown>;
  if (typeof func.switch === "function") {
    // js-xdr class API
    ic = (func as { invokeContract: () => Record<string, unknown> }).invokeContract() as Record<string, unknown>;
  } else {
    // plain-object XDR API
    ic = (func.invokeContract ?? func.value) as Record<string, unknown>;
  }
  const get = <T>(key: string): T => {
    const v = ic[key];
    return (typeof v === "function" ? (v as () => T).call(ic) : (v as T)) as T;
  };
  const scAddress = get<Parameters<typeof Address.fromScAddress>[0]>("contractAddress");
  const name = get<unknown>("functionName");
  const args = get<unknown[]>("args");
  return {
    contractId: Address.fromScAddress(scAddress).toString(),
    method: String(name),
    argCount: Array.isArray(args) ? args.length : 0,
  };
}
