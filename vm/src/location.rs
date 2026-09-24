use std::fmt::{self, Debug, Display, Formatter};

#[derive(Clone, Copy, Debug)]
pub struct Coords(u16, u16);

impl Coords {
    pub fn new(row: u16, col: u16) -> Self {
        Self(row, col)
    }

    pub fn locate<T>(&self, value: T) -> AtCoords<T> {
        AtCoords {
            coords: *self,
            value,
        }
    }

    pub fn row(&self) -> u16 {
        self.0
    }

    pub fn col(&self) -> u16 {
        self.1
    }
}

impl Display for Coords {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}

#[derive(Clone, Debug)]
pub struct AtCoords<T> {
    coords: Coords,
    pub value: T,
}

impl<T> AtCoords<T> {
    pub fn at(row: u16, col: u16, value: T) -> Self {
        Self {
            coords: Coords(row, col),
            value,
        }
    }

    pub fn coords(&self) -> Coords {
        self.coords
    }

    pub fn co_locate<A>(&self, value: A) -> AtCoords<A> {
        AtCoords::at(self.coords.0, self.coords.1, value)
    }
}

impl<T: Display> Display for AtCoords<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.coords, self.value)
    }
}

#[derive(Clone, Debug)]
pub enum AtCoordsOrEof<T> {
    AtCoords(AtCoords<T>),
    Eof(T),
}

impl<T> From<AtCoords<T>> for AtCoordsOrEof<T> {
    fn from(value: AtCoords<T>) -> Self {
        Self::AtCoords(value)
    }
}

impl<T: Display> Display for AtCoordsOrEof<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtCoords(v) => write!(f, "{}", v),
            Self::Eof(v) => write!(f, "eof: {}", v),
        }
    }
}

impl<T> AtCoordsOrEof<T> {
    pub fn at_coords(row: u16, col: u16, value: T) -> Self {
        Self::AtCoords(AtCoords::at(row, col, value))
    }

    pub fn at_eof(value: T) -> Self {
        Self::Eof(value)
    }

    pub fn value(&self) -> &T {
        match self {
            Self::AtCoords(v) => &v.value,
            Self::Eof(v) => v,
        }
    }
}

/*
impl<T: Clone> Clone for Located<T> {
    fn clone(&self) -> Self {
        Self {
            pos: self.pos.clone(),
            value: self.value.clone(),
        }
    }
}

impl<E: Error> Display for Located<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pos, self.value)
    }
}

impl<E: Error> Error for Located<E> {}*/
