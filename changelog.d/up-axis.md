### Breaking

- A CADIR 11 document can hold a root `up_axis` (`"y"` or `"z"`), the axis that points up in the source document. A Fusion decode writes it, and a Fusion write stores it. An earlier build does not know the key. A document whose source states no up axis writes no key.
