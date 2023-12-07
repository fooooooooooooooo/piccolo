use gc_arena::Collect;
use piccolo::{
    AnyCallback, AnySequence, CallbackReturn, Closure, Context, Error, Execution, Executor,
    Function, IntoValue, Lua, Sequence, SequencePoll, Stack, StaticError, String, Thread, Value,
};

#[test]
fn callback() -> Result<(), StaticError> {
    let mut lua = Lua::core();

    lua.try_enter(|ctx| {
        let callback = AnyCallback::from_fn(&ctx, |_, _, mut stack| {
            stack.push_back(Value::Integer(42));
            Ok(CallbackReturn::Return)
        });
        ctx.state.globals.set(ctx, "callback", callback)?;
        Ok(())
    })?;

    let executor = lua.try_enter(|ctx| {
        let closure = Closure::load(
            ctx,
            None,
            &br#"
                local a, b, c = callback(1, 2)
                assert(a == 1 and b == 2 and c == 42)
                local d, e, f = callback(3, 4)
                assert(d == 3 and e == 4 and f == 42)
            "#[..],
        )?;

        Ok(ctx
            .state
            .registry
            .stash(&ctx, Executor::start(ctx, closure.into(), ())))
    })?;

    lua.execute::<()>(&executor)?;
    Ok(())
}

#[test]
fn tail_call_trivial_callback() -> Result<(), StaticError> {
    let mut lua = Lua::core();

    lua.try_enter(|ctx| {
        let callback = AnyCallback::from_fn(&ctx, |_, _, mut stack| {
            stack.push_back(Value::Integer(3));
            Ok(CallbackReturn::Return)
        });
        ctx.state.globals.set(ctx, "callback", callback)?;
        Ok(())
    })?;

    let executor = lua.try_enter(|ctx| {
        let closure = Closure::load(
            ctx,
            None,
            &br#"
                return callback(1, 2)
            "#[..],
        )?;

        Ok(ctx
            .state
            .registry
            .stash(&ctx, Executor::start(ctx, closure.into(), ())))
    })?;

    assert_eq!(lua.execute::<(i64, i64, i64)>(&executor)?, (1, 2, 3));
    Ok(())
}

#[test]
fn loopy_callback() -> Result<(), StaticError> {
    let mut lua = Lua::core();

    lua.try_enter(|ctx| {
        let callback = AnyCallback::from_fn(&ctx, |ctx, _, _| {
            #[derive(Collect)]
            #[collect(require_static)]
            struct Cont(i64);

            impl<'gc> Sequence<'gc> for Cont {
                fn poll(
                    &mut self,
                    _ctx: Context<'gc>,
                    _exec: Execution<'gc, '_>,
                    mut stack: Stack<'gc, '_>,
                ) -> Result<SequencePoll<'gc>, Error<'gc>> {
                    stack.push_back(self.0.into());
                    self.0 += 1;
                    if self.0 > 6 {
                        Ok(SequencePoll::Return)
                    } else {
                        Ok(SequencePoll::Pending)
                    }
                }
            }

            Ok(CallbackReturn::Call {
                function: AnyCallback::from_fn(&ctx, |_, _, mut stack| {
                    stack.push_back(3.into());
                    Ok(CallbackReturn::Yield {
                        to_thread: None,
                        then: None,
                    })
                })
                .into(),
                then: Some(AnySequence::new(&ctx, Cont(4))),
            })
        });
        ctx.state.globals.set(ctx, "callback", callback)?;
        Ok(())
    })?;

    let executor = lua.try_enter(|ctx| {
        let closure = Closure::load(
            ctx,
            None,
            &br#"
                local function cotest()
                    return callback(1, 2)
                end

                local co = coroutine.create(cotest)

                local e1, r1, r2, r3 = coroutine.resume(co)
                local s1 = coroutine.status(co)
                local e2, r4, r5, r6, r7, r8, r9 = coroutine.resume(co, r1, r2, r3)
                local s2 = coroutine.status(co)

                return
                    e1 == true and
                    r1 == 1 and r2 == 2 and r3 == 3 and
                    s1 == "suspended" and
                    e2 == true and
                    r4 == 1 and r5 == 2 and r6 == 3 and r7 == 4 and r8 == 5 and r9 == 6 and
                    s2 == "dead"
            "#[..],
        )?;

        Ok(ctx
            .state
            .registry
            .stash(&ctx, Executor::start(ctx, closure.into(), ())))
    })?;

    assert!(lua.execute::<bool>(&executor)?);
    Ok(())
}

#[test]
fn yield_sequence() -> Result<(), StaticError> {
    let mut lua = Lua::core();

    lua.try_enter(|ctx| {
        let callback = AnyCallback::from_fn(&ctx, |ctx, _, mut stack| {
            #[derive(Collect)]
            #[collect(require_static)]
            struct Cont(i8);

            impl<'gc> Sequence<'gc> for Cont {
                fn poll(
                    &mut self,
                    ctx: Context<'gc>,
                    _exec: Execution<'gc, '_>,
                    mut stack: Stack<'gc, '_>,
                ) -> Result<SequencePoll<'gc>, Error<'gc>> {
                    match self.0 {
                        0 => {
                            let (a, b): (i32, i32) = stack.consume(ctx)?;
                            assert_eq!((a, b), (5, 6));
                            stack.extend([Value::Integer(7), Value::Integer(8)]);
                            self.0 = 1;
                            Ok(SequencePoll::Yield {
                                to_thread: None,
                                is_tail: false,
                            })
                        }
                        1 => {
                            let (a, b): (i32, i32) = stack.consume(ctx)?;
                            assert_eq!((a, b), (9, 10));
                            stack.extend([Value::Integer(11), Value::Integer(12)]);
                            self.0 = 2;
                            Ok(SequencePoll::Return)
                        }
                        _ => unreachable!(),
                    }
                }
            }

            let (a, b): (i32, i32) = stack.consume(ctx)?;
            assert_eq!((a, b), (1, 2));
            stack.extend([Value::Integer(3), Value::Integer(4)]);
            Ok(CallbackReturn::Yield {
                to_thread: None,
                then: Some(AnySequence::new(&ctx, Cont(0))),
            })
        });
        ctx.state.globals.set(ctx, "callback", callback)?;
        Ok(())
    })?;

    let executor = lua.try_enter(|ctx| {
        let closure = Closure::load(
            ctx,
            None,
            &br#"
                local co = coroutine.create(callback)

                local e, r1, r2 = coroutine.resume(co, 1, 2)
                assert(e == true and r1 == 3 and r2 == 4)
                assert(coroutine.status(co) == "suspended")

                local e, r1, r2 = coroutine.resume(co, 5, 6)
                assert(e == true and r1 == 7 and r2 == 8)
                assert(coroutine.status(co) == "suspended")

                local e, r1, r2 = coroutine.resume(co, 9, 10)
                assert(e == true and r1 == 11 and r2 == 12)
                assert(coroutine.status(co) == "dead")
            "#[..],
        )?;

        Ok(ctx
            .state
            .registry
            .stash(&ctx, Executor::start(ctx, closure.into(), ())))
    })?;

    lua.execute(&executor)
}

#[test]
fn resume_with_err() {
    let mut lua = Lua::core();

    let executor = lua.enter(|ctx| {
        let callback = AnyCallback::from_fn(&ctx, |ctx, _, mut stack| {
            #[derive(Collect)]
            #[collect(require_static)]
            struct Cont;

            impl<'gc> Sequence<'gc> for Cont {
                fn poll(
                    &mut self,
                    _ctx: Context<'gc>,
                    _exec: Execution<'gc, '_>,
                    _stack: Stack<'gc, '_>,
                ) -> Result<SequencePoll<'gc>, Error<'gc>> {
                    panic!("did not error");
                }

                fn error(
                    &mut self,
                    ctx: Context<'gc>,
                    _exec: Execution<'gc, '_>,
                    _error: Error<'gc>,
                    _stack: Stack<'gc, '_>,
                ) -> Result<SequencePoll<'gc>, Error<'gc>> {
                    Err("a different error".into_value(ctx).into())
                }
            }

            assert!(stack.len() == 1);
            assert_eq!(stack.consume::<String>(ctx)?, "resume");
            stack.replace(ctx, "return");
            Ok(CallbackReturn::Yield {
                to_thread: None,
                then: Some(AnySequence::new(&ctx, Cont)),
            })
        });

        let thread = Thread::new(ctx);
        thread
            .start_suspended(&ctx, Function::Callback(callback))
            .unwrap();

        thread.resume(ctx, "resume").unwrap();

        ctx.state.registry.stash(&ctx, Executor::run(&ctx, thread))
    });

    lua.finish(&executor);

    lua.enter(|ctx| {
        let executor = ctx.state.registry.fetch(&executor);
        assert!(executor.take_result::<String>(ctx).unwrap().unwrap() == "return");
        executor
            .resume_err(&ctx, "an error".into_value(ctx).into())
            .unwrap();
    });

    lua.finish(&executor);

    lua.enter(|ctx| {
        let executor = ctx.state.registry.fetch(&executor);
        match executor.take_result::<()>(ctx).unwrap() {
            Err(Error::Lua(val)) => {
                assert!(matches!(val.0, Value::String(s) if s == "a different error"))
            }
            _ => panic!("wrong error returned"),
        }
    });
}
