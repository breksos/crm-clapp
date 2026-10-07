import { createContext, useContext } from "react";
import { LOCAL, type Identity } from "./identity";

/** Who is at this seat, for everything that draws an attribution. A context rather than a prop
 *  threaded through every timeline, feed and card: it is window chrome, not record data. */
export const IdentityContext = createContext<Identity>(LOCAL);
export const useIdentity = () => useContext(IdentityContext);
