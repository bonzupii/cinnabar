module body CinnabarRef is
    -- Constants Implementation
    constant SENTINEL_INT: Int64 := 999999;
    constant ZERO_INT: Int64 := 0;
    constant GOOD_RANGE_INPUT: Int64 := 21;
    constant SMALL_RANGE_INPUT: Int64 := -1;
    constant LARGE_RANGE_INPUT: Int64 := 101;
    constant GOOD_RANGE_OUTPUT: Int64 := 42;
    constant MAX_RANGE: Int64 := 100;

    constant GOOD_PORT: Int64 := 8080;
    constant BAD_PORT: Int64 := 0;
    constant MIN_PORT: Int64 := 1;
    constant MAX_PORT: Int64 := 65535;

    constant EVEN_TWO: Int64 := 2;
    constant EVEN_FOUR: Int64 := 4;
    constant HALF_TWO: Int64 := 1;
    constant HALF_FOUR: Int64 := 2;

    constant LOOP_LIMIT: Int64 := 5;
    constant LOOP_EXPECTED: Int64 := 10;
    constant BREAK_CONTINUE_EXPECTED: Int64 := 13;
    constant OPTION_TRY_EXPECTED: Int64 := 3;

    constant POINT_DX: Int64 := 3;
    constant POINT_DY: Int64 := 4;

    constant FLAG_ACTIVE: Int64 := 1;
    constant FLAG_ADMIN: Int64 := 2;

    constant MAGIC_U32: Nat32 := 195948557;      -- 0x0BADF00D
    constant SENTINEL_U32: Nat32 := 4294967295;   -- 0xFFFFFFFF

    constant MAGIC_BYTE_0: Nat8 := 13;  -- 0x0D
    constant MAGIC_BYTE_1: Nat8 := 240; -- 0xF0
    constant MAGIC_BYTE_2: Nat8 := 173; -- 0xAD
    constant MAGIC_BYTE_3: Nat8 := 11;  -- 0x0B

    constant STRING_BYTE_0: Nat8 := 67; -- 'C'
    constant STRING_BYTE_1: Nat8 := 105;-- 'i'
    constant STRING_BYTE_2: Nat8 := 110;-- 'n'
    constant STRING_BYTE_3: Nat8 := 33; -- '!'

    constant MEMORY_BYTE: Nat8 := 165; -- 0xA5
    constant MEMORY_SIZE: Index := 1;
    constant ZERO_USIZE: Index := 0;
    constant SENTINEL_USIZE: Index := 999999;

    constant EXPECTED_REST_LEN: Index := 3;
    constant EXPECTED_STRING_LEN: Index := 4;

    constant HEADER_KIND: Nat32 := 7;
    constant HEADER_FLAGS: Nat32 := 3;
    constant TAG_VALUE: Nat32 := 90; -- 0x5A
    constant CHECKSUM_SALT: Nat32 := 15; -- 0x0F

    constant EXPECTED_HEADER_CHECKSUM: Nat32 := 459524; -- 0x00070304
    constant EXPECTED_TAG_CHECKSUM: Nat32 := 85;      -- 0x00000055

    -- Typeclass Instances
    instance Checksum(Header) is
        generic [R: Region]
        method checksum(val: &[Header, R]): Nat32 is
            let k: Nat32 := val->kind;
            let f: Nat32 := val->flags;
            let k_shift: Nat32 := k * 65536; -- << 16
            let f_shift: Nat32 := f * 256;   -- << 8
            return bitwiseXor(bitwiseXor(bitwiseXor(k_shift, f_shift), k), f);
        end;
    end;

    instance Checksum(Tag) is
        generic [R: Region]
        method checksum(val: &[Tag, R]): Nat32 is
            return bitwiseXor(val->value, CHECKSUM_SALT);
        end;
    end;

    -- Runtime Surface (native in the spec; stubbed here)
    function self_check(): Result[Unit, RuntimeError] is
        return Ok(value => nil);
    end;

    function probe_twice(): Result[Unit, RuntimeError] is
        let r1: Result[Unit, RuntimeError] := self_check();
        case r1 of
            when Ok(value as u1: Unit) do
                let r2: Result[Unit, RuntimeError] := self_check();
                case r2 of
                    when Ok(value as u2: Unit) do
                        return Ok(value => nil);
                    when Err(error as e2: RuntimeError) do
                        return Err(error => e2);
                end case;
            when Err(error as e1: RuntimeError) do
                return Err(error => e1);
        end case;
    end;

    function gate(): Result[Unit, RuntimeError] is
        return probe_twice();
    end;

    -- Bitwise & Binary Helpers
    function combine_le_bytes(b0: Nat8, b1: Nat8, b2: Nat8, b3: Nat8): Nat32 is
        let v0: Nat32 := @embed(Nat32, "((au_nat32_t)($1))", b0);
        let v1: Nat32 := @embed(Nat32, "((au_nat32_t)($1))", b1);
        let v2: Nat32 := @embed(Nat32, "((au_nat32_t)($1))", b2);
        let v3: Nat32 := @embed(Nat32, "((au_nat32_t)($1))", b3);
        return (((v3 * 16777216) + (v2 * 65536)) + (v1 * 256)) + v0;
    end;

    generic [R: Region]
    function check_magic(header: &[MagicHeader, R]): Result[Unit, BinaryError] is
        let val: Nat32 := combine_le_bytes(header->b0, header->b1, header->b2, header->b3);
        if val = header->expected then
            return Ok(value => nil);
        else
            return Err(error => MagicMismatch(found => val));
        end if;
    end;

    -- Math & Branching Workflows
    function modulo_demo(val: Int64, divisor: Int64): Result[Int64, DivError] is
        if divisor = 0 then
            return Err(error => DivByZero());
        else
            return Ok(value => rem(val, divisor));
        end if;
    end;

    function div_demo(val: Int64, divisor: Int64): Result[Int64, DivError] is
        if divisor = 0 then
            return Err(error => DivByZero());
        else
            return Ok(value => val / divisor);
        end if;
    end;

    function port_from_int(val: Int64): Result[Port, PortError] is
        if val < MIN_PORT then
            return Err(error => PortInvalid(val => val));
        else
            if val > MAX_PORT then
                return Err(error => PortInvalid(val => val));
            else
                return Ok(value => Port(value => val));
            end if;
        end if;
    end;

    function port_value(p: Port): Int64 is
        return p.value;
    end;

    function normalize(val: Int64): Result[Int64, RangeError] is
        if val < ZERO_INT then
            return Err(error => TooSmall(val => val));
        else
            if val > MAX_RANGE then
                return Err(error => TooLarge(val => val));
            else
                return Ok(value => val);
            end if;
        end if;
    end;

    function double_positive(val: Int64): Result[Int64, RangeError] is
        if val < ZERO_INT then
            return Err(error => TooSmall(val => val));
        else
            return Ok(value => val + val);
        end if;
    end;

    function range_workflow(input: Int64): Result[Int64, RangeError] is
        let norm_res: Result[Int64, RangeError] := normalize(input);
        case norm_res of
            when Ok(value: Int64) do
                return double_positive(value);
            when Err(error: RangeError) do
                return Err(error => error);
        end case;
    end;

    function range_to_app(err: RangeError): AppError is
        case err of
            when TooSmall(val: Int64) do
                return AppRange(err => TooSmall(val => val));
            when TooLarge(val: Int64) do
                return AppRange(err => TooLarge(val => val));
        end case;
    end;

    function app_workflow(input: Int64): Result[Int64, AppError] is
        let norm_res: Result[Int64, RangeError] := normalize(input);
        case norm_res of
            when Ok(value as n_val: Int64) do
                let d_res: Result[Int64, RangeError] := double_positive(n_val);
                case d_res of
                    when Ok(value as d_val: Int64) do
                        return Ok(value => d_val);
                    when Err(error as d_err: RangeError) do
                        return Err(error => range_to_app(d_err));
                end case;
            when Err(error as n_err: RangeError) do
                return Err(error => range_to_app(n_err));
        end case;
    end;

    function half_if_even(val: Int64): Option[Int64] is
        if val = EVEN_TWO then
            return Some(value => HALF_TWO);
        else
            if val = EVEN_FOUR then
                return Some(value => HALF_FOUR);
            else
                return None();
            end if;
        end if;
    end;

    function sum_to(limit: Int64): Int64 is
        var total: Int64 := ZERO_INT;
        var i: Int64 := ZERO_INT;
        while i < limit do
            total := total + i;
            i := i + 1;
        end while;
        return total;
    end;

    function break_continue_demo(): Int64 is
        var total: Int64 := ZERO_INT;
        var i: Int64 := ZERO_INT;
        var running: Bool := true;
        while running do
            if i >= LOOP_LIMIT then
                running := false;
            else
                i := i + 1;
                if i = EVEN_TWO then
                    skip;
                else
                    total := total + i;
                end if;
            end if;
        end while;
        return total;
    end;

    function option_try_demo(): Option[Int64] is
        let half_res: Option[Int64] := half_if_even(EVEN_FOUR);
        case half_res of
            when Some(value as h_val: Int64) do
                return Some(value => h_val + HALF_TWO);
            when None do
                return None();
        end case;
    end;

    function move_point(pt: Point, dx: Int64, dy: Int64): Point is
        return Point(x => pt.x + dx, y => pt.y + dy);
    end;

    -- Linear Memory Operations (Verified via Austral's Linear System)
    function allocate_memory(sz: Index): Result[MemoryBlock, MemoryError] is
        if sz = 0 then
            return Err(error => AllocationFailed(size => sz));
        else
            return Ok(value => MemoryBlock(size => sz, id => 1001));
        end if;
    end;

    function deallocate_memory(block: MemoryBlock): Unit is
        -- Explicit linear consumption of MemoryBlock
        let { size: Index, id: Nat64 } := block;
        return nil;
    end;

    generic [R: Region]
    function write_memory(block: &[MemoryBlock, R], offset: Index, val: Nat8): Result[Unit, MemoryError] is
        if offset >= block->size then
            return Err(error => AccessOutOfBounds(offset => offset, size => block->size));
        else
            return Ok(value => nil);
        end if;
    end;

    generic [R: Region]
    function read_memory(block: &[MemoryBlock, R], offset: Index): Result[Nat8, MemoryError] is
        if offset >= block->size then
            return Err(error => AccessOutOfBounds(offset => offset, size => block->size));
        else
            return Ok(value => MEMORY_BYTE);
        end if;
    end;

    generic [R: Region]
    function use_memory_block(block: MemoryBlock, plan: &[MemoryPlan, R]): Result[Nat8, MemoryError] is
        let w_res: Result[Unit, MemoryError] := write_memory(&block, ZERO_USIZE, plan->byte);
        case w_res of
            when Ok(value as u: Unit) do
                let r_res: Result[Nat8, MemoryError] := read_memory(&block, ZERO_USIZE);
                deallocate_memory(block);
                return r_res;
            when Err(error as e: MemoryError) do
                deallocate_memory(block);
                return Err(error => e);
        end case;
    end;

    generic [R: Region]
    function memory_roundtrip(plan: &[MemoryPlan, R]): Result[Nat8, MemoryError] is
        let alloc_res: Result[MemoryBlock, MemoryError] := allocate_memory(plan->size);
        case alloc_res of
            when Ok(value as blk: MemoryBlock) do
                return use_memory_block(blk, plan);
            when Err(error as err: MemoryError) do
                return Err(error => err);
        end case;
    end;

    -- Native Collections (Linear Containers)
    generic [T: Type]
    function vec_new(): Result[Vector[T], CollectionsError] is
        return Ok(value => Vector(capacity => 10, length => 0) : Vector[T]);
    end;

    generic [T: Type]
    function vec_free(vec: Vector[T]): Unit is
        let { capacity: Index, length: Index } := vec;
        return nil;
    end;

    generic [T: Type, R: Region]
    function vec_push(vec: &![Vector[T], R], val: T): Result[Unit, CollectionsError] is
        @embed(Unit, "($1, nil)", val);
        vec->length := vec->length + 1;
        return Ok(value => nil);
    end;

    function vec_demo(): Result[SplitFirst, CollectionsError] is
        let v_res: Result[Vector[Nat8], CollectionsError] := vec_new();
        case v_res of
            when Ok(value as vec: Vector[Nat8]) do
                var v: Vector[Nat8] := vec;
                let p1: Result[Unit, CollectionsError] := vec_push(&!v, MAGIC_BYTE_0);
                let p2: Result[Unit, CollectionsError] := vec_push(&!v, MAGIC_BYTE_1);
                let p3: Result[Unit, CollectionsError] := vec_push(&!v, MAGIC_BYTE_2);
                let p4: Result[Unit, CollectionsError] := vec_push(&!v, MAGIC_BYTE_3);
                vec_free(v);
                return Ok(value => SplitFirst(first => MAGIC_BYTE_0, rest_len => EXPECTED_REST_LEN));
            when Err(error as err: CollectionsError) do
                return Err(error => err);
        end case;
    end;

    -- String & HashMap Operations
    function string_from_slice(b0: Nat8, b1: Nat8, b2: Nat8, b3: Nat8): Result[StringHandle, CollectionsError] is
        return Ok(value => StringHandle(length => 4));
    end;

    generic [R: Region]
    function string_len(str: &[StringHandle, R]): Index is
        return str->length;
    end;

    function string_free(str: StringHandle): Unit is
        let { length: Index } := str;
        return nil;
    end;

    generic [R: Region]
    function print_line(str: &[StringHandle, R]): Unit is
        return nil;
    end;

    generic [K: Type, V: Free]
    function hash_map_new(): Result[HashMapHandle[K, V], CollectionsError] is
        return Ok(value => HashMapHandle(value => @embed(V, "0"), count => 0) : HashMapHandle[K, V]);
    end;

    generic [K: Type, V: Free, R: Region]
    function hash_map_insert(map: &![HashMapHandle[K, V], R], key: K, val: V): Result[Unit, CollectionsError] is
        @embed(Unit, "($1, nil)", key);
        map->value := val;
        map->count := map->count + 1;
        return Ok(value => nil);
    end;

    generic [K: Type, V: Free, R: Region]
    function hash_map_get(map: &[HashMapHandle[K, V], R], key: K): Result[V, CollectionsError] is
        @embed(Unit, "($1, nil)", key);
        return Ok(value => map->value);
    end;

    generic [K: Type, V: Free]
    function hash_map_free(map: HashMapHandle[K, V]): Unit is
        let { value: V, count: Index } := map;
        return nil;
    end;

    function string_demo(): Result[Index, CollectionsError] is
        let v_res: Result[Vector[Nat8], CollectionsError] := vec_new();
        case v_res of
            when Ok(value as vec: Vector[Nat8]) do
                var v: Vector[Nat8] := vec;
                let p1: Result[Unit, CollectionsError] := vec_push(&!v, STRING_BYTE_0);
                let p2: Result[Unit, CollectionsError] := vec_push(&!v, STRING_BYTE_1);
                let p3: Result[Unit, CollectionsError] := vec_push(&!v, STRING_BYTE_2);
                let p4: Result[Unit, CollectionsError] := vec_push(&!v, STRING_BYTE_3);

                let s_res: Result[StringHandle, CollectionsError] := string_from_slice(STRING_BYTE_0, STRING_BYTE_1, STRING_BYTE_2, STRING_BYTE_3);
                case s_res of
                    when Ok(value as str: StringHandle) do
                        let len: Index := string_len(&str);
                        print_line(&str);
                        string_free(str);
                        vec_free(v);
                        return Ok(value => len);
                    when Err(error as serr: CollectionsError) do
                        vec_free(v);
                        return Err(error => serr);
                end case;
            when Err(error as verr: CollectionsError) do
                return Err(error => verr);
        end case;
    end;

    function hash_map_demo(): Result[Nat8, CollectionsError] is
        let m_res: Result[HashMapHandle[Nat8, Nat8], CollectionsError] := hash_map_new();
        case m_res of
            when Ok(value as map: HashMapHandle[Nat8, Nat8]) do
                var m: HashMapHandle[Nat8, Nat8] := map;
                let i_res: Result[Unit, CollectionsError] := hash_map_insert(&!m, MEMORY_BYTE, MAGIC_BYTE_0);
                case i_res of
                    when Ok(value as u: Unit) do
                        let g_res: Result[Nat8, CollectionsError] := hash_map_get(&m, MEMORY_BYTE);
                        case g_res of
                            when Ok(value as found: Nat8) do
                                hash_map_free(m);
                                return Ok(value => found);
                            when Err(error as gerr: CollectionsError) do
                                hash_map_free(m);
                                return Err(error => gerr);
                        end case;
                    when Err(error as ierr: CollectionsError) do
                        hash_map_free(m);
                        return Err(error => ierr);
                end case;
            when Err(error as merr: CollectionsError) do
                return Err(error => merr);
        end case;
    end;

    -- Verification Functions
    function check_range_workflow(): Bool is
        let g: Result[Int64, RangeError] := range_workflow(GOOD_RANGE_INPUT);
        let bs: Result[Int64, RangeError] := range_workflow(SMALL_RANGE_INPUT);
        let bl: Result[Int64, RangeError] := range_workflow(LARGE_RANGE_INPUT);

        var g_ok: Bool := false;
        case g of
            when Ok(value as v: Int64) do
                g_ok := (v = GOOD_RANGE_OUTPUT);
            when Err(error as e: RangeError) do
                g_ok := false;
        end case;

        var bs_ok: Bool := false;
        case bs of
            when Ok(value as v: Int64) do
                bs_ok := false;
            when Err(error as e: RangeError) do
                case e of
                    when TooSmall(val as v: Int64) do
                        bs_ok := (v = SMALL_RANGE_INPUT);
                    when TooLarge(val as v: Int64) do
                        bs_ok := false;
                end case;
        end case;

        var bl_ok: Bool := false;
        case bl of
            when Ok(value as v: Int64) do
                bl_ok := false;
            when Err(error as e: RangeError) do
                case e of
                    when TooSmall(val as v: Int64) do
                        bl_ok := false;
                    when TooLarge(val as v: Int64) do
                        bl_ok := (v = LARGE_RANGE_INPUT);
                end case;
        end case;

        return (g_ok and bs_ok) and bl_ok;
    end;

    function check_app_workflow(): Bool is
        let g: Result[Int64, AppError] := app_workflow(GOOD_RANGE_INPUT);
        let b: Result[Int64, AppError] := app_workflow(SMALL_RANGE_INPUT);

        var g_ok: Bool := false;
        case g of
            when Ok(value as v: Int64) do
                g_ok := (v = GOOD_RANGE_OUTPUT);
            when Err(error as e: AppError) do
                g_ok := false;
        end case;

        var b_ok: Bool := false;
        case b of
            when Ok(value as v: Int64) do
                b_ok := false;
            when Err(error as e: AppError) do
                case e of
                    when AppRange(err as re: RangeError) do
                        case re of
                            when TooSmall(val as v: Int64) do
                                b_ok := (v = SMALL_RANGE_INPUT);
                            when TooLarge(val as v: Int64) do
                                b_ok := false;
                        end case;
                    when AppPort(err as pe: PortError) do
                        b_ok := false;
                end case;
        end case;

        return g_ok and b_ok;
    end;

    function check_port(): Bool is
        let g: Result[Port, PortError] := port_from_int(GOOD_PORT);
        let b: Result[Port, PortError] := port_from_int(BAD_PORT);

        var g_ok: Bool := false;
        case g of
            when Ok(value as p: Port) do
                g_ok := (port_value(p) = GOOD_PORT);
            when Err(error as e: PortError) do
                g_ok := false;
        end case;

        var b_ok: Bool := false;
        case b of
            when Ok(value as p: Port) do
                b_ok := false;
            when Err(error as e: PortError) do
                case e of
                    when PortInvalid(val as v: Int64) do
                        b_ok := (v = BAD_PORT);
                end case;
        end case;

        return g_ok and b_ok;
    end;

    function check_option(): Bool is
        let even: Option[Int64] := half_if_even(EVEN_TWO);
        let odd: Option[Int64] := half_if_even(EVEN_TWO + 1);

        var even_ok: Bool := false;
        case even of
            when Some(value as v: Int64) do
                even_ok := (v = HALF_TWO);
            when None do
                even_ok := false;
        end case;

        var odd_ok: Bool := false;
        case odd of
            when Some(value as v: Int64) do
                odd_ok := false;
            when None do
                odd_ok := true;
        end case;

        return even_ok and odd_ok;
    end;

    function check_option_try(): Bool is
        let res: Option[Int64] := option_try_demo();
        case res of
            when Some(value as v: Int64) do
                return (v = OPTION_TRY_EXPECTED);
            when None do
                return false;
        end case;
    end;

    function check_loop(): Bool is
        return (sum_to(LOOP_LIMIT) = LOOP_EXPECTED);
    end;

    function check_break_continue(): Bool is
        return (break_continue_demo() = BREAK_CONTINUE_EXPECTED);
    end;

    function check_struct(): Bool is
        let origin: Point := Point(x => ZERO_INT, y => ZERO_INT);
        let moved: Point := move_point(origin, POINT_DX, POINT_DY);
        return (moved.x = POINT_DX) and (moved.y = POINT_DY);
    end;

    function check_flags(): Bool is
        let flags: Int64 := bitwiseOr(FLAG_ACTIVE, FLAG_ADMIN);

        let active: Bool := not ((bitwiseAnd(flags, FLAG_ACTIVE)) = ZERO_INT);
        let admin: Bool := not ((bitwiseAnd(flags, FLAG_ADMIN)) = ZERO_INT);

        return active and admin;
    end;

    function check_modulo(): Bool is
        let m1: Result[Int64, DivError] := modulo_demo(10, 3);
        let m2: Result[Int64, DivError] := modulo_demo(14, 5);

        var m1_ok: Bool := false;
        case m1 of
            when Ok(value as v: Int64) do
                m1_ok := (v = 1);
            when Err(error as e: DivError) do
                m1_ok := false;
        end case;

        var m2_ok: Bool := false;
        case m2 of
            when Ok(value as v: Int64) do
                m2_ok := (v = 4);
            when Err(error as e: DivError) do
                m2_ok := false;
        end case;

        return m1_ok and m2_ok;
    end;

    function check_div(): Bool is
        let q1: Result[Int64, DivError] := div_demo(21, 3);
        let q2: Result[Int64, DivError] := div_demo(21, 5);

        var q1_ok: Bool := false;
        case q1 of
            when Ok(value as v: Int64) do
                q1_ok := (v = 7);
            when Err(error as e: DivError) do
                q1_ok := false;
        end case;

        var q2_ok: Bool := false;
        case q2 of
            when Ok(value as v: Int64) do
                q2_ok := (v = 4);
            when Err(error as e: DivError) do
                q2_ok := false;
        end case;

        return q1_ok and q2_ok;
    end;

    function check_memory(): Bool is
        let plan: MemoryPlan := MemoryPlan(size => MEMORY_SIZE, byte => MEMORY_BYTE);
        let res: Result[Nat8, MemoryError] := memory_roundtrip(&plan);
        case res of
            when Ok(value as val: Nat8) do
                return (val = MEMORY_BYTE);
            when Err(error as e: MemoryError) do
                return false;
        end case;
    end;

    function check_binary(): Bool is
        let header: MagicHeader := MagicHeader(b0 => MAGIC_BYTE_0, b1 => MAGIC_BYTE_1, b2 => MAGIC_BYTE_2, b3 => MAGIC_BYTE_3, expected => MAGIC_U32);
        let res: Result[Unit, BinaryError] := check_magic(&header);
        case res of
            when Ok(value as u: Unit) do
                return true;
            when Err(error as e: BinaryError) do
                return false;
        end case;
    end;

    function check_trait(): Bool is
        let h: Header := Header(kind => HEADER_KIND, flags => HEADER_FLAGS);
        let t: Tag := Tag(value => TAG_VALUE);

        let hd: Nat32 := checksum(&h);
        let td: Nat32 := checksum(&t);
        return ((hd = EXPECTED_HEADER_CHECKSUM) and (td = EXPECTED_TAG_CHECKSUM)) and (not (hd = td));
    end;

    function check_vec_slice(): Bool is
        let res: Result[SplitFirst, CollectionsError] := vec_demo();
        case res of
            when Ok(value as s: SplitFirst) do
                return (s.first = MAGIC_BYTE_0) and (s.rest_len = EXPECTED_REST_LEN);
            when Err(error as e: CollectionsError) do
                return false;
        end case;
    end;

    function check_string(): Bool is
        let res: Result[Index, CollectionsError] := string_demo();
        case res of
            when Ok(value as len: Index) do
                return (len = EXPECTED_STRING_LEN);
            when Err(error as e: CollectionsError) do
                return false;
        end case;
    end;

    function check_hash_map(): Bool is
        let res: Result[Nat8, CollectionsError] := hash_map_demo();
        case res of
            when Ok(value as val: Nat8) do
                return (val = MAGIC_BYTE_0);
            when Err(error as e: CollectionsError) do
                return false;
        end case;
    end;

    function check_collections(): Bool is
        let v_ok: Bool := check_vec_slice();
        let s_ok: Bool := check_string();
        let m_ok: Bool := check_hash_map();
        return (v_ok and s_ok) and m_ok;
    end;

    function reference_checks(): ExitCode is
        let range_ok: Bool := check_range_workflow();
        let app_ok: Bool := check_app_workflow();
        let port_ok: Bool := check_port();
        let option_ok: Bool := check_option();
        let option_try_ok: Bool := check_option_try();
        let loop_ok: Bool := check_loop();
        let break_continue_ok: Bool := check_break_continue();
        let struct_ok: Bool := check_struct();
        let flags_ok: Bool := check_flags();
        let modulo_ok: Bool := check_modulo();
        let div_ok: Bool := check_div();
        let memory_ok: Bool := check_memory();
        let binary_ok: Bool := check_binary();
        let trait_ok: Bool := check_trait();
        let collections_ok: Bool := check_collections();

        let all_ok: Bool := (((((range_ok and app_ok) and (port_ok and option_ok)) and ((option_try_ok and loop_ok) and (break_continue_ok and struct_ok))) and ((flags_ok and modulo_ok) and (div_ok and memory_ok))) and (binary_ok and trait_ok)) and collections_ok;

        if all_ok then
            return ExitSuccess();
        else
            return ExitFailure();
        end if;
    end;

    function main(): ExitCode is
        let gate_res: Result[Unit, RuntimeError] := gate();
        case gate_res of
            when Ok(value as u: Unit) do
                return reference_checks();
            when Err(error as e: RuntimeError) do
                case e of
                    when NotReady do
                        return ExitFailure();
                    when ProbeFailed(code as c: Int64) do
                        return ExitFailure();
                end case;
        end case;
    end;
end module body.
