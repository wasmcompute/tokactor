/// Convert a frunk HList into a nested tuple list. To be able to convert an HList
/// into a nested tuple, it is required for the struct to implement IntoNestedTuple.
///
/// Implementations should look like the following:
///
/// ```
/// struct Bool(pub bool);
///
/// impl<H> IntoListTuple<H> for Bool {
///     type TupleList = (Self, H);
///
///     fn into_tuple_list(self, inner: H) -> Self::TupleList {
///         (self, inner)
///     }
/// }
/// ```
///
pub trait IntoNestedTuple<Inner> {
    type TupleList: Tuple;

    fn into_tuple_list(self, inner: Inner) -> Self::TupleList;
}

impl<Inner: Tuple> IntoNestedTuple<Inner> for () {
    type TupleList = Inner;

    fn into_tuple_list(self, list: Inner) -> Self::TupleList {
        list
    }
}

impl<Inner, Head, Tail> IntoNestedTuple<Inner> for (Head, Tail)
where
    Head: IntoNestedTuple<Tail::TupleList>,
    Tail: IntoNestedTuple<Inner>,
{
    type TupleList = Head::TupleList;

    fn into_tuple_list(self, list: Inner) -> Self::TupleList {
        self.0.into_tuple_list(self.1.into_tuple_list(list))
    }
}

/// A trait to flatten out a nested tuple into a flat tuple. This is mostly
/// an internal implementation to attempt at creating a callable tuple. This
/// should be generated through macros but it's easier to just code it all.
pub trait Tuple {
    type Output;

    fn flatten(self) -> Self::Output;
}

impl Tuple for () {
    type Output = ();

    fn flatten(self) -> Self::Output {}
}

impl<T1> Tuple for (T1, ()) {
    type Output = (T1,);

    fn flatten(self) -> Self::Output {
        (self.0,)
    }
}

impl<T1, T2> Tuple for (T1, (T2, ())) {
    type Output = (T2, T1);

    fn flatten(self) -> Self::Output {
        (self.1 .0, self.0)
    }
}

impl<T1, T2, T3> Tuple for (T1, (T2, (T3, ()))) {
    type Output = (T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (self.1 .1 .0, self.1 .0, self.0)
    }
}

impl<T1, T2, T3, T4> Tuple for (T1, (T2, (T3, (T4, ())))) {
    type Output = (T4, T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (self.1 .1 .1 .0, self.1 .1 .0, self.1 .0, self.0)
    }
}

impl<T1, T2, T3, T4, T5> Tuple for (T1, (T2, (T3, (T4, (T5, ()))))) {
    type Output = (T5, T4, T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (
            self.1 .1 .1 .1 .0,
            self.1 .1 .1 .0,
            self.1 .1 .0,
            self.1 .0,
            self.0,
        )
    }
}

impl<T1, T2, T3, T4, T5, T6> Tuple for (T1, (T2, (T3, (T4, (T5, (T6, ())))))) {
    type Output = (T6, T5, T4, T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (
            self.1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .0,
            self.1 .1 .1 .0,
            self.1 .1 .0,
            self.1 .0,
            self.0,
        )
    }
}

impl<T1, T2, T3, T4, T5, T6, T7> Tuple for (T1, (T2, (T3, (T4, (T5, (T6, (T7, ()))))))) {
    type Output = (T7, T6, T5, T4, T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (
            self.1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .0,
            self.1 .1 .1 .0,
            self.1 .1 .0,
            self.1 .0,
            self.0,
        )
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8> Tuple for (T1, (T2, (T3, (T4, (T5, (T6, (T7, (T8, ())))))))) {
    type Output = (T8, T7, T6, T5, T4, T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (
            self.1 .1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .0,
            self.1 .1 .1 .0,
            self.1 .1 .0,
            self.1 .0,
            self.0,
        )
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9> Tuple
    for (T1, (T2, (T3, (T4, (T5, (T6, (T7, (T8, (T9, ())))))))))
{
    type Output = (T9, T8, T7, T6, T5, T4, T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (
            self.1 .1 .1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .0,
            self.1 .1 .1 .0,
            self.1 .1 .0,
            self.1 .0,
            self.0,
        )
    }
}

impl<T1, T2, T3, T4, T5, T6, T7, T8, T9, T10> Tuple
    for (
        T1,
        (T2, (T3, (T4, (T5, (T6, (T7, (T8, (T9, (T10, ()))))))))),
    )
{
    type Output = (T10, T9, T8, T7, T6, T5, T4, T3, T2, T1);

    fn flatten(self) -> Self::Output {
        (
            self.1 .1 .1 .1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .1 .0,
            self.1 .1 .1 .1 .0,
            self.1 .1 .1 .0,
            self.1 .1 .0,
            self.1 .0,
            self.0,
        )
    }
}
